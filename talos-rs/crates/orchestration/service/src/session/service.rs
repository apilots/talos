use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use talos_config::{provider_config_for_llm, LlmConfig};
use talos_error::{TalosError, TalosResult};
use talos_store::project::ProjectStore;
use talos_store::session::{
    SessionPatchAck, SessionQuery, SessionRecord, SessionStore, SessionTreeIdentity,
    SessionTreeScope,
};
use talos_types::talos::{TalosConnectionMode, TalosContext};
use talos_types::{
    connection_mode_from_context, normalize_current_directory, resolve_default_model_selection,
    resolve_effective_project_root, AgentTurnRef, ModelInfo, ModelSelection,
    OptionalCurrentDirectoryPatch, SessionConfigOverridePolicy, SessionContext, SessionPatch,
    SessionState, TalosConfig, TalosSession, DEFAULT_SESSION_TITLE,
};

use super::CreateSessionParams;

const API_SESSION_SCOPE: &str = "api";

#[derive(Clone)]
pub struct SessionService {
    session_store: Arc<dyn SessionStore>,
    project_store: Arc<dyn ProjectStore>,
    base_config: Arc<RwLock<TalosConfig>>,
}

impl SessionService {
    pub fn new(
        session_store: Arc<dyn SessionStore>,
        project_store: Arc<dyn ProjectStore>,
        base_config: TalosConfig,
    ) -> Self {
        Self {
            session_store,
            project_store,
            base_config: Arc::new(RwLock::new(base_config)),
        }
    }

    fn base_model_selection(&self) -> TalosResult<ModelSelection> {
        let base_config = self.base_config_snapshot();
        resolve_default_model_selection(&base_config.llm)
    }

    fn validate_model_selection(&self, selection: &ModelSelection) -> TalosResult<()> {
        let llm = &self.base_config_snapshot().llm;
        let model = provider_config_for_llm(llm, &selection.model_ref.provider)
            .and_then(|provider| {
                provider
                    .models
                    .iter()
                    .find(|model| model.id == selection.model_ref.id)
            })
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ProviderUnavailable,
                    format!(
                        "{}:{}",
                        selection.model_ref.provider, selection.model_ref.id
                    ),
                )
            })?;
        selection.resolve_reasoning(&ModelInfo::from_provider_model(
            selection.model_ref.provider.clone(),
            model.clone(),
        ))?;
        Ok(())
    }

    fn validate_persisted_model_selection(&self, record: &SessionRecord) -> TalosResult<()> {
        record.conf.model_selection.as_ref().ok_or_else(|| {
            TalosError::internal(format!(
                "session '{}' uses an incompatible persisted session format: missing modelSelection",
                record.id
            ))
        })?;
        SessionConfigOverridePolicy::validate_conf(&record.conf)
    }

    fn base_config_snapshot(&self) -> TalosConfig {
        self.base_config
            .read()
            .expect("session base config read lock")
            .clone()
    }

    pub fn update_base_llm(&self, llm: &LlmConfig) {
        let mut base_config = self
            .base_config
            .write()
            .expect("session base config write lock");
        base_config.llm = llm.clone();
    }

    pub fn update_base_config(&self, config: TalosConfig) {
        *self
            .base_config
            .write()
            .expect("session base config write lock") = config;
    }

    /// Return the Workspace selected by the current product routing policy.
    pub fn default_workspace_id(&self) -> String {
        self.base_config_snapshot()
            .default_workspace_id()
            .to_string()
    }

    fn build_session(&self, record: &SessionRecord) -> TalosResult<TalosSession> {
        let base_config = self.base_config_snapshot();
        let effective_user_permission_mode =
            SessionConfigOverridePolicy::resolve_user_permission_mode(&base_config, &record.conf);
        let config = SessionConfigOverridePolicy::apply_to_config(&base_config, &record.conf)?;
        Ok(TalosSession {
            agent_id: record.agent_id.clone(),
            title: record.title.clone(),
            user_id: record.user_id.clone(),
            context: SessionContext {
                session_id: record.id.clone(),
                workspace_id: record.workspace_id.clone(),
                source_type: record.context.source_type.clone(),
                provider: record.context.provider.clone(),
                user_id: record.context.user_id.clone(),
                message_id: record.context.message_id.clone(),
                trace_id: record.context.trace_id.clone(),
                channel_id: record.context.channel_id.clone(),
                thread_id: record.context.thread_id.clone(),
                project_id: record.project_id.clone(),
                current_directory: record.current_directory.clone(),
                project_root: None,
                environment: record.context.environment.clone(),
            },
            conf: record.conf.clone(),
            project_id: record.project_id.clone(),
            current_directory: record.current_directory.clone(),
            config,
            effective_user_permission_mode,
            state: SessionState {
                created_at: record.created_at,
                last_activity: record.last_activity,
                message_count: record.message_count,
                usage: record.usage.clone(),
            },
        })
    }

    async fn resolve_effective_root(&self, session: &TalosSession) -> TalosResult<Option<PathBuf>> {
        let project_directory = if let Some(project_id) = session.project_id.as_deref() {
            Some(
                self.project_store
                    .get_project(project_id)
                    .await?
                    .ok_or_else(|| {
                        TalosError::public(
                            talos_error::ErrorCode::ResourceNotFound,
                            format!("project {}", project_id),
                        )
                    })?
                    .directory,
            )
        } else {
            None
        };
        let workspace_root = session
            .config
            .resolve_workspace(&session.context.workspace_id, &session.user_id)?
            .root;
        Ok(resolve_effective_project_root(
            session,
            project_directory,
            workspace_root.exists().then_some(workspace_root),
        ))
    }

    async fn validate_project_binding(&self, project_id: &str, user_id: &str) -> TalosResult<()> {
        let project = self
            .project_store
            .get_project(project_id)
            .await?
            .filter(|project| project.user_id == user_id)
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("project {project_id}"),
                )
            })?;
        if project.project_id.trim().is_empty() {
            return Err(TalosError::internal(
                "project store returned an empty project_id".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn create_session(&self, params: CreateSessionParams) -> TalosResult<TalosSession> {
        self.create_session_with_tree_identity(params, SessionTreeIdentity::Primary)
            .await
    }

    pub(crate) async fn create_subagent_session(
        &self,
        params: CreateSessionParams,
        parent: AgentTurnRef,
        tree_root_session_id: String,
        agent_path: talos_types::AgentPath,
    ) -> TalosResult<TalosSession> {
        self.create_session_with_tree_identity(
            params,
            SessionTreeIdentity::Subagent {
                parent,
                tree_root_session_id,
                path: agent_path,
            },
        )
        .await
    }

    async fn create_session_with_tree_identity(
        &self,
        mut params: CreateSessionParams,
        tree_identity: SessionTreeIdentity,
    ) -> TalosResult<TalosSession> {
        if let Some(project_id) = params.project_id.as_deref() {
            self.validate_project_binding(project_id, &params.context.user_id)
                .await?;
        }
        params.context.project_root = None;
        params.current_directory =
            normalize_current_directory(&params.context, params.current_directory)?;
        let record = self.build_session_record(params, tree_identity)?;
        let mut session = self.build_session(&record)?;
        session.context.project_root = self.resolve_effective_root(&session).await?;
        self.session_store
            .insert_session_record(&record, API_SESSION_SCOPE)
            .await?;
        Ok(session)
    }

    pub async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
        let records = self.session_store.list_session_records(&query).await?;
        let mut sessions = Vec::with_capacity(records.len());
        for record in &records {
            self.validate_persisted_model_selection(record)?;
            let mut session = self.build_session(record)?;
            session.context.project_root = self.resolve_effective_root(&session).await?;
            sessions.push(session);
        }
        Ok(sessions)
    }

    pub async fn count_sessions(&self, query: SessionQuery) -> TalosResult<usize> {
        self.session_store.count_session_records(&query).await
    }

    pub async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        self.load_session(session_id).await
    }

    pub(crate) async fn get_session_record(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<SessionRecord>> {
        self.session_store.get_session_record(session_id).await
    }

    pub(crate) async fn get_subagent_record(
        &self,
        tree_root_session_id: &str,
        agent_path: &talos_types::AgentPath,
    ) -> TalosResult<Option<SessionRecord>> {
        self.session_store
            .get_subagent_record(tree_root_session_id, agent_path)
            .await
    }

    pub(crate) async fn list_subagent_tree_records(
        &self,
        tree_root_session_id: String,
        limit: Option<usize>,
    ) -> TalosResult<Vec<SessionRecord>> {
        self.session_store
            .list_session_records(&SessionQuery {
                tree_scope: SessionTreeScope::SubagentTree {
                    tree_root_session_id,
                },
                limit,
                ..SessionQuery::default()
            })
            .await
    }

    pub(crate) async fn has_active_subagent_turn(
        &self,
        tree_root_session_id: &str,
    ) -> TalosResult<bool> {
        self.session_store
            .has_active_subagent_turn(tree_root_session_id)
            .await
    }

    pub async fn load_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        let Some(record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(None);
        };
        self.validate_persisted_model_selection(&record)?;
        let mut session = self.build_session(&record)?;
        session.context.project_root = self.resolve_effective_root(&session).await?;
        Ok(Some(session))
    }

    pub async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let Some(record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };
        self.validate_persisted_model_selection(&record)?;
        let patch = self.normalize_session_patch(record.context.clone(), patch)?;
        self.apply_session_patch(record, patch).await
    }

    pub async fn reset_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };
        self.validate_persisted_model_selection(&record)?;
        record.title = DEFAULT_SESSION_TITLE.to_string();
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        record.message_count = 0;
        record.usage = Default::default();
        let success = self.session_store.update_session_record(&record).await?;
        Ok(SessionPatchAck { success })
    }

    pub(crate) async fn record_compaction_usage(
        &self,
        session_id: &str,
        assistant_model: &str,
        usage: &talos_types::talos::TokenUsage,
    ) -> TalosResult<()> {
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        };
        self.validate_persisted_model_selection(&record)?;
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        record.usage.tokens.accumulate(usage);
        record.model = assistant_model.to_string();
        self.session_store.update_session_record(&record).await?;
        Ok(())
    }

    pub(crate) async fn record_persisted_turn_progress(
        &self,
        session_id: &str,
        artifact_seq: u64,
        assistant_model: &str,
        usage: &talos_types::talos::TokenUsage,
    ) -> TalosResult<()> {
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        };
        self.validate_persisted_model_selection(&record)?;
        if artifact_seq <= record.latest_turn_seq {
            return Ok(());
        }
        let persisted_turn_delta = artifact_seq - record.latest_turn_seq;
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        record.message_count = record
            .message_count
            .saturating_add(persisted_turn_delta.saturating_mul(2));
        record.latest_turn_seq = artifact_seq;
        record.usage.tokens.accumulate(usage);
        record.model = assistant_model.to_string();
        self.session_store.update_session_record(&record).await?;
        Ok(())
    }

    pub async fn update_session_title(&self, session_id: &str, title: &str) -> TalosResult<()> {
        let normalized = title.trim();
        if normalized.is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "session title must not be empty".to_string(),
            ));
        }
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        };
        self.validate_persisted_model_selection(&record)?;
        record.title = normalized.to_string();
        if !self.session_store.update_session_record(&record).await? {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        }
        Ok(())
    }

    pub async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let record = self
            .session_store
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("session {session_id}"),
                )
            })?;
        if matches!(record.tree_identity, SessionTreeIdentity::Subagent { .. }) {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("session '{session_id}' is controlled by its parent agent"),
            ));
        }
        let success = self.session_store.delete_session_tree(session_id).await?;
        Ok(SessionPatchAck { success })
    }

    pub(crate) async fn delete_subagent_session(
        &self,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        let record = self
            .session_store
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("session {session_id}"),
                )
            })?;
        if !matches!(record.tree_identity, SessionTreeIdentity::Subagent { .. }) {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("session '{session_id}' is not a subagent session"),
            ));
        }
        let success = self.session_store.delete_session_record(session_id).await?;
        Ok(SessionPatchAck { success })
    }

    fn normalize_session_patch(
        &self,
        context: TalosContext,
        mut patch: SessionPatch,
    ) -> TalosResult<SessionPatch> {
        if let Some(OptionalCurrentDirectoryPatch::Set { path }) = patch.current_directory.as_ref()
        {
            patch.current_directory = normalize_current_directory(&context, Some(path.clone()))?
                .map(|normalized| OptionalCurrentDirectoryPatch::Set { path: normalized });
        }
        if matches!(
            connection_mode_from_context(&context),
            TalosConnectionMode::Remote
        ) {
            patch.current_directory = None;
        }
        Ok(patch)
    }

    fn build_session_record(
        &self,
        params: CreateSessionParams,
        tree_identity: SessionTreeIdentity,
    ) -> TalosResult<SessionRecord> {
        self.base_config_snapshot()
            .resolve_workspace(&params.workspace_id, &params.context.user_id)?;
        let session_id = params
            .session_id
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let requested_selection = params.model_selection;
        let effective_selection = match requested_selection {
            Some(selection) => selection,
            None => self.base_model_selection()?,
        };
        let base_conf =
            SessionConfigOverridePolicy::single_model_selection_conf(&effective_selection);
        let conf = params
            .conf
            .as_ref()
            .map(|patch_conf| SessionConfigOverridePolicy::merge_conf(&base_conf, patch_conf))
            .unwrap_or(base_conf);
        SessionConfigOverridePolicy::validate_conf(&conf)?;
        let stored_selection = SessionConfigOverridePolicy::extract_model_selection(&conf)?
            .unwrap_or(effective_selection);
        self.validate_model_selection(&stored_selection)?;
        let stored_model = stored_selection.model_ref.id;
        let now = chrono::Utc::now().timestamp_millis();
        let record = SessionRecord {
            id: session_id,
            agent_id: params.target_agent,
            tree_identity,
            workspace_id: params.workspace_id,
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: params.context.user_id.clone(),
            context: params.context,
            model: stored_model,
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            latest_turn_seq: 0,
            memory_processed_until_seq: 0,
            usage: Default::default(),
            conf,
            project_id: params.project_id,
            current_directory: params.current_directory,
        };
        Ok(record)
    }

    async fn apply_session_patch(
        &self,
        mut record: SessionRecord,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let previous_selection =
            SessionConfigOverridePolicy::extract_model_selection(&record.conf)?;
        if let Some(conf) = patch.conf.as_ref() {
            SessionConfigOverridePolicy::validate_conf(conf)?;
        }
        let patch_conf = patch.conf.as_ref().cloned().unwrap_or_default();
        record.conf = SessionConfigOverridePolicy::merge_conf(&record.conf, &patch_conf);
        let next_selection = SessionConfigOverridePolicy::extract_model_selection(&record.conf)?;
        if let Some(selection) = next_selection.as_ref() {
            self.validate_model_selection(selection)?;
            record.model = selection.model_ref.id.clone();
        }
        if next_selection != previous_selection {
            record.usage.context = None;
        }
        if let Some(current_directory_patch) = &patch.current_directory {
            match current_directory_patch {
                OptionalCurrentDirectoryPatch::Set { path } => {
                    record.current_directory = Some(path.clone());
                }
                OptionalCurrentDirectoryPatch::Clear => {
                    record.current_directory = None;
                }
            }
        }
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        let success = self.session_store.update_session_record(&record).await?;
        Ok(SessionPatchAck { success })
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
