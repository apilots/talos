use std::collections::HashMap;
use std::sync::Arc;

use talos_agent::{AgentDefinition, InnerModelClient};
use talos_config::{
    resolve_effective_kernel_workspace, AgentKernelConfig, AgentRuntimeConfig,
    DreamMemoryMaintenanceConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_memory::MemoryScope;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, SessionArtifactStore, SessionPageCursor, SessionQuery,
    SessionStore,
};
use talos_types::ModelUsageKind;

use super::MemoryService;

/// Result of one bounded global Dream Memory maintenance run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DreamMemoryMaintenanceReport {
    pub processed_sessions: u64,
    pub failed_sessions: u64,
}

/// Processes bounded session tails through the shared memory formation path.
#[derive(Clone)]
pub struct DreamMemoryMaintenance {
    session_store: Arc<dyn SessionStore>,
    active_turn_store: Arc<dyn ActiveTurnStore>,
    session_artifact_store: Arc<dyn SessionArtifactStore>,
    memory_service: MemoryService,
    inner_model: Arc<InnerModelClient>,
    agent_kernel_configs: Arc<HashMap<String, AgentKernelConfig>>,
    config: DreamMemoryMaintenanceConfig,
}

impl DreamMemoryMaintenance {
    pub fn new(
        session_store: Arc<dyn SessionStore>,
        active_turn_store: Arc<dyn ActiveTurnStore>,
        session_artifact_store: Arc<dyn SessionArtifactStore>,
        memory_service: MemoryService,
        inner_model: Arc<InnerModelClient>,
        agent_definitions: &[AgentDefinition],
        config: DreamMemoryMaintenanceConfig,
    ) -> Self {
        let agent_kernel_configs = agent_definitions
            .iter()
            .filter(|definition| definition.enabled)
            .map(|definition| match &definition.runtime_config {
                AgentRuntimeConfig::Kernel { kernel } => (definition.id.clone(), kernel.clone()),
            })
            .collect();
        Self {
            session_store,
            active_turn_store,
            session_artifact_store,
            memory_service,
            inner_model,
            agent_kernel_configs: Arc::new(agent_kernel_configs),
            config,
        }
    }

    pub async fn run(&self) -> TalosResult<DreamMemoryMaintenanceReport> {
        let mut report = DreamMemoryMaintenanceReport::default();
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
                match self.process_session(record).await {
                    Ok(true) => report.processed_sessions += 1,
                    Ok(false) => {}
                    Err(error) => {
                        report.failed_sessions += 1;
                        tracing::warn!(session_id, error = %error, "Dream Memory session processing failed");
                    }
                }
            }
            if record_count < self.config.batch_size {
                break;
            }
        }
        Ok(report)
    }

    async fn process_session(
        &self,
        record: talos_store::session::SessionRecord,
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
        let agent_kernel_config =
            self.agent_kernel_configs
                .get(&record.agent_id)
                .ok_or_else(|| {
                    TalosError::public(
                        talos_error::ErrorCode::ResourceNotFound,
                        format!("Dream Memory Agent not found: {}", record.agent_id),
                    )
                })?;
        let kernel_config =
            resolve_effective_kernel_workspace(agent_kernel_config, &record.user_id)?.kernel_config;
        let workspace_root = std::path::Path::new(&kernel_config.workspace_dir);
        if self
            .session_artifact_store
            .latest_seq(workspace_root, &record.id)
            .await?
            <= record.memory_processed_until_seq
        {
            return Ok(false);
        }
        let provider = self.inner_model.scoped_provider(
            record.user_id.clone(),
            Some(record.id.clone()),
            ModelUsageKind::Dream,
        )?;
        let selected_model = self.inner_model.model_ref().id;
        let scope = MemoryScope {
            agent_id: record.agent_id,
            user_id: record.user_id,
            workspace_root: std::path::PathBuf::from(&kernel_config.workspace_dir),
        };
        let result = talos_agent_kernel::form_dream_session_memory(
            talos_agent_kernel::MemoryFormationParams {
                session_id: &record.id,
                selected_model: &selected_model,
                llm_provider: provider,
                dream_memory_store: &self.memory_service,
                memory_scope: &scope,
                session_artifact_store: self.session_artifact_store.as_ref(),
                session_store: self.session_store.as_ref(),
                kernel_config: &kernel_config,
            },
        )
        .await?;
        Ok(result.covers_until_seq.is_some())
    }
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
