use std::collections::HashMap;
use std::sync::Arc;

use talos_config::{resolve_effective_kernel_workspace, MemoryDailyMaintenanceConfig};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProviderRouter};
use talos_memory::MemoryScope;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, SessionArtifactStore, SessionPageCursor, SessionQuery,
    SessionStore,
};
use talos_types::{
    ModelSelection, SessionConfigOverridePolicy, TalosConfig, MEMORY_FILE_MAX_CHARS,
};

use super::MemoryService;

const MANAGED_START: &str = "<!-- talos:managed-memory:start -->";
const MANAGED_END: &str = "<!-- talos:managed-memory:end -->";
const HIGH_MEMORY_PROMPT_MAX_CHARS: usize = 6_000;
const MANAGED_MEMORY_PROMPT: &str = r#"Rewrite the managed long-term memory section from the supplied high-importance memories.
Return only concise Markdown bullet points. Preserve durable facts, preferences, decisions, and open tasks. Do not include markers or code fences."#;

/// Result of one bounded global daily-memory maintenance run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryDailyMaintenanceReport {
    pub processed_sessions: u64,
    pub failed_sessions: u64,
    pub updated_users: u64,
    pub failed_users: u64,
}

/// Processes bounded session tails through the shared memory formation path.
#[derive(Clone)]
pub struct MemoryDailyMaintenance {
    session_store: Arc<dyn SessionStore>,
    active_turn_store: Arc<dyn ActiveTurnStore>,
    session_artifact_store: Arc<dyn SessionArtifactStore>,
    memory_service: MemoryService,
    llm_provider_router: Arc<dyn LlmProviderRouter>,
    base_config: TalosConfig,
    config: MemoryDailyMaintenanceConfig,
}

impl MemoryDailyMaintenance {
    pub fn new(
        session_store: Arc<dyn SessionStore>,
        active_turn_store: Arc<dyn ActiveTurnStore>,
        session_artifact_store: Arc<dyn SessionArtifactStore>,
        memory_service: MemoryService,
        llm_provider_router: Arc<dyn LlmProviderRouter>,
        base_config: TalosConfig,
        config: MemoryDailyMaintenanceConfig,
    ) -> Self {
        Self {
            session_store,
            active_turn_store,
            session_artifact_store,
            memory_service,
            llm_provider_router,
            base_config,
            config,
        }
    }

    pub async fn run(&self) -> TalosResult<MemoryDailyMaintenanceReport> {
        let mut report = MemoryDailyMaintenanceReport::default();
        let mut user_models = HashMap::new();
        let mut after = None;
        for _ in 0..self.config.max_batches {
            let records = self
                .session_store
                .list_session_records(&SessionQuery {
                    limit: Some(self.config.batch_size),
                    after: after.clone(),
                    ..SessionQuery::default()
                })
                .await?;
            let record_count = records.len();
            after = records.last().map(|record| SessionPageCursor {
                last_activity: record.last_activity,
                session_id: record.id.clone(),
            });
            for record in records {
                let session_id = record.id.clone();
                let selection = match SessionConfigOverridePolicy::extract_model_selection(
                    &record.conf,
                ) {
                    Ok(Some(selection)) => selection,
                    Err(error) => {
                        report.failed_sessions += 1;
                        tracing::warn!(session_id, error = %error, "daily memory session configuration is invalid");
                        continue;
                    }
                    Ok(None) => {
                        report.failed_sessions += 1;
                        tracing::warn!(session_id = %record.id, "daily memory session is missing model selection");
                        continue;
                    }
                };
                user_models
                    .entry(record.user_id.clone())
                    .or_insert_with(|| selection.clone());
                match self.process_session(record, &selection).await {
                    Ok(true) => report.processed_sessions += 1,
                    Ok(false) => {}
                    Err(error) => {
                        report.failed_sessions += 1;
                        tracing::warn!(session_id, error = %error, "daily memory session processing failed");
                    }
                }
            }
            if record_count < self.config.batch_size {
                break;
            }
        }
        for (user_id, selection) in user_models {
            match self.refresh_managed_memory(&user_id, &selection).await {
                Ok(true) => report.updated_users += 1,
                Ok(false) => {}
                Err(error) => {
                    report.failed_users += 1;
                    tracing::warn!(user_id, error = %error, "managed memory update failed");
                }
            }
        }
        Ok(report)
    }

    async fn process_session(
        &self,
        record: talos_store::session::SessionRecord,
        selection: &ModelSelection,
    ) -> TalosResult<bool> {
        if matches!(
            self.active_turn_store
                .get_active_turn(&record.id)
                .await?
                .map(|turn| turn.status),
            Some(
                ActiveTurnStatus::Running
                    | ActiveTurnStatus::WaitingUserAction
                    | ActiveTurnStatus::Cancelling
            )
        ) {
            return Ok(false);
        }
        let kernel_config =
            resolve_effective_kernel_workspace(&self.base_config.agent.kernel, &record.user_id)?
                .kernel_config;
        let workspace_root = std::path::Path::new(&kernel_config.workspace_dir);
        if self
            .session_artifact_store
            .latest_seq(workspace_root, &record.id)
            .await?
            <= record.memory_processed_until_seq
        {
            return Ok(false);
        }
        let provider = self
            .llm_provider_router
            .provider_for(&selection.model_ref)?;
        let scope = MemoryScope {
            user_id: record.user_id,
            workspace_root: std::path::PathBuf::from(&kernel_config.workspace_dir),
        };
        let result = talos_agent_kernel::form_daily_session_memory(
            talos_agent_kernel::MemoryFormationParams {
                session_id: &record.id,
                selected_model: &selection.model_ref.id,
                llm_provider: provider,
                memory_writer: &self.memory_service,
                memory_scope: &scope,
                session_artifact_store: self.session_artifact_store.as_ref(),
                session_store: self.session_store.as_ref(),
                kernel_config: &kernel_config,
            },
        )
        .await?;
        Ok(result.covers_until_seq.is_some())
    }

    async fn refresh_managed_memory(
        &self,
        user_id: &str,
        selection: &ModelSelection,
    ) -> TalosResult<bool> {
        let memories = self.memory_service.recent_high(user_id).await?;
        if memories.is_empty() {
            return Ok(false);
        }
        let kernel_config =
            resolve_effective_kernel_workspace(&self.base_config.agent.kernel, user_id)?
                .kernel_config;
        let path = std::path::PathBuf::from(&kernel_config.workspace_dir)
            .join(&self.base_config.memory.root_memory_filename);
        let current = talos_fs::read_file(&path).await?.unwrap_or_default();
        let existing_managed = managed_content(&current)?;
        let source = render_high_memories(&memories);
        let provider = self
            .llm_provider_router
            .provider_for(&selection.model_ref)?;
        let response = provider
            .chat(
                ChatRequest::builder()
                    .model(selection.model_ref.id.clone())
                    .messages(vec![
                        ChatMessage::system(MANAGED_MEMORY_PROMPT),
                        ChatMessage::user(format!(
                            "Current managed memory:\n{existing_managed}\n\nHigh-importance memories:\n{source}"
                        )),
                    ])
                    .build(),
            )
            .await?;
        let managed = response.content.trim();
        if managed.is_empty() || managed.chars().count() > MEMORY_FILE_MAX_CHARS {
            return Err(TalosError::ValidationError(
                "managed memory output is empty or too large".to_string(),
            ));
        }
        let updated = replace_managed_content(&current, managed)?;
        talos_fs::with_file_lock(&path, || async {
            let latest = talos_fs::read_file(&path).await?.unwrap_or_default();
            if latest != current {
                return Err(TalosError::Conflict(
                    "MEMORY.md changed during managed update".to_string(),
                ));
            }
            talos_fs::write_file(&path, &updated).await
        })
        .await?;
        Ok(true)
    }
}

fn render_high_memories(memories: &[talos_types::MemoryInfo]) -> String {
    let mut output = String::new();
    for memory in memories {
        let entry = format!("- {}: {}\n", memory.title.trim(), memory.body.trim());
        if output.chars().count() + entry.chars().count() > HIGH_MEMORY_PROMPT_MAX_CHARS {
            break;
        }
        output.push_str(&entry);
    }
    output
}

fn managed_content(content: &str) -> TalosResult<&str> {
    let starts = content.match_indices(MANAGED_START).collect::<Vec<_>>();
    let ends = content.match_indices(MANAGED_END).collect::<Vec<_>>();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(""),
        ([(start, _)], [(end, _)]) if start < end => {
            Ok(content[start + MANAGED_START.len()..*end].trim())
        }
        _ => Err(TalosError::ValidationError(
            "MEMORY.md has invalid managed section markers".to_string(),
        )),
    }
}

fn replace_managed_content(content: &str, managed: &str) -> TalosResult<String> {
    managed_content(content)?;
    let section = format!("{MANAGED_START}\n{managed}\n{MANAGED_END}");
    let updated = match (content.find(MANAGED_START), content.find(MANAGED_END)) {
        (Some(start), Some(end)) => {
            let suffix = end + MANAGED_END.len();
            format!("{}{}{}", &content[..start], section, &content[suffix..])
        }
        (None, None) if content.trim().is_empty() => format!("{section}\n"),
        (None, None) => format!("{}\n\n{section}\n", content.trim_end()),
        _ => unreachable!("managed_content validates marker pairs"),
    };
    if updated.chars().count() > MEMORY_FILE_MAX_CHARS {
        return Err(TalosError::ValidationError(
            "MEMORY.md exceeds the 8000 character limit".to_string(),
        ));
    }
    Ok(updated)
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
