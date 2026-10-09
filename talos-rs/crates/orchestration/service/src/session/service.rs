use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use talos_agent::{AgentContext, AgentSessionContext};
use talos_agent_api::AgentTurnRef;
use talos_config::provider_config_for_llm;
use talos_error::{TalosError, TalosResult};
use talos_store::project::ProjectStore;
use talos_store::session::{
    AccumulateSessionUsageParams, PatchSessionParams, SessionQuery, SessionRecord, SessionStore,
    SessionTreeIdentity, SessionTreeScope,
};
use talos_types::talos::TalosConnectionMode;
use talos_types::{
    normalize_current_directory, resolve_default_model_selection, ModelInfo, ModelSelection,
    OptionalCurrentDirectoryPatch, SessionConfigOverridePolicy, SessionInfo, SessionPatch,
    TalosConfig, UserPermissionMode, DEFAULT_SESSION_TITLE,
};

use super::CreateSessionParams;

#[path = "title.rs"]
mod title;

#[derive(Clone)]
pub(crate) struct SessionService {
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

    fn model_info_for_selection(&self, selection: &ModelSelection) -> TalosResult<ModelInfo> {
        let llm = &self.base_config_snapshot().llm;
        let model = provider_config_for_llm(llm, &selection.model_ref.provider)
            .and_then(|provider| {
                provider
                    .models
                    .iter()
                    .find(|model| model.id == selection.model_ref.id)
            })
            .ok_or_else(|| {
                TalosError::localized(talos_error::ErrorLocalization::ModelNotConfigured {
                    provider: selection.model_ref.provider.clone(),
                    model: selection.model_ref.id.clone(),
                })
            })?;
        let model =
            ModelInfo::from_provider_model(selection.model_ref.provider.clone(), model.clone());
        selection.resolve_reasoning(&model)?;
        Ok(model)
    }

    fn validate_persisted_model_selection(&self, record: &SessionRecord) -> TalosResult<()> {
        record.conf.model_selection.as_ref().ok_or_else(|| {
            TalosError::internal("Session uses an incompatible persisted session format: missing modelSelection.").context(format!(
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

    pub fn update_base_config(&self, config: TalosConfig) {
        *self
            .base_config
            .write()
            .expect("session base config write lock") = config;
    }

    pub(crate) fn kernel_timezone(&self) -> String {
        self.base_config_snapshot().agent.kernel.timezone
    }

    /// Return the Workspace selected by the current product routing policy.
    pub fn default_workspace_id(&self) -> String {
        self.base_config_snapshot()
            .default_workspace_id()
            .to_string()
    }

    pub(crate) fn session_info(&self, record: &SessionRecord) -> TalosResult<SessionInfo> {
        let base_config = self.base_config_snapshot();
        let model_selection = record.conf.model_selection.clone().ok_or_else(|| {
            TalosError::internal("Session is missing its model selection.").context(format!(
                "session '{}' is missing its model selection",
                record.id
            ))
        })?;
        let effective_mode =
            SessionConfigOverridePolicy::resolve_user_permission_mode(&base_config, &record.conf);
        Ok(SessionInfo {
            id: record.id.clone(),
            title: record.title.clone(),
            user_id: record.user_id.clone(),
            source_type: record.source_type.clone(),
            source_provider: record.source_provider.clone(),
            model_selection,
            configured_default_mode: UserPermissionMode::from(
                &base_config
                    .agent
                    .kernel
                    .tools
                    .sandbox_policy
                    .user_permission_mode,
            ),
            effective_mode,
            created_at: record.created_at,
            last_activity: record.last_activity,
            message_count: record.message_count,
            usage: record.usage.clone(),
            project_id: record.project_id.clone(),
            current_directory: record.current_directory.clone(),
        })
    }

    async fn resolve_effective_root(&self, record: &SessionRecord) -> TalosResult<Option<PathBuf>> {
        let project_directory = if let Some(project_id) = record.project_id.as_deref() {
            Some(
                self.project_store
                    .get_project(project_id)
                    .await?
                    .ok_or_else(|| {
                        TalosError::new(
                            talos_error::ErrorCode::ResourceNotFound,
                            format!("Project {} was not found.", project_id),
                        )
                    })?
                    .directory,
            )
        } else {
            None
        };
        if project_directory.is_some() {
            return Ok(project_directory);
        }
        if record.environment.as_ref().is_some_and(|environment| {
            environment.connection_mode == talos_types::talos::TalosConnectionMode::Embedded
        }) && record.current_directory.is_some()
        {
            return Ok(record.current_directory.clone());
        }
        // The execution node resolves the implicit Workspace from its own configuration.
        Ok(None)
    }

    async fn validate_project_binding(&self, project_id: &str, user_id: &str) -> TalosResult<()> {
        let project = self
            .project_store
            .get_project(project_id)
            .await?
            .filter(|project| project.user_id == user_id)
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("Project {project_id} was not found."),
                )
            })?;
        if project.project_id.trim().is_empty() {
            return Err(TalosError::internal(
                "project store returned an empty project_id",
            ));
        }
        Ok(())
    }

    pub(crate) async fn create_session(
        &self,
        params: CreateSessionParams,
    ) -> TalosResult<SessionRecord> {
        self.create_session_with_tree_identity(params, SessionTreeIdentity::Primary)
            .await
    }

    pub(crate) async fn create_subagent_session(
        &self,
        params: CreateSessionParams,
        parent: AgentTurnRef,
        tree_root_session_id: String,
        agent_path: talos_types::AgentPath,
    ) -> TalosResult<SessionRecord> {
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
    ) -> TalosResult<SessionRecord> {
        if let Some(project_id) = params.project_id.as_deref() {
            self.validate_project_binding(project_id, &params.user_id)
                .await?;
        }
        let connection_mode = params
            .source
            .environment
            .as_ref()
            .map(|environment| environment.connection_mode)
            .unwrap_or_default();
        params.current_directory =
            normalize_current_directory(connection_mode, params.current_directory)?;
        let record = self.build_session_record(params, tree_identity)?;
        self.session_store.insert_session_record(&record).await?;
        Ok(record)
    }

    pub(crate) async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<SessionInfo>> {
        let records = self.session_store.list_session_records(&query).await?;
        let mut sessions = Vec::with_capacity(records.len());
        for record in &records {
            self.validate_persisted_model_selection(record)?;
            sessions.push(self.session_info(record)?);
        }
        Ok(sessions)
    }

    /// Build the unique trusted execution Context for one logical Agent Turn.
    ///
    /// This Service boundary owns product binding and policy resolution. The
    /// returned value crosses Coordinator, Harness, and Provider unchanged and
    /// must never be persisted as a second Session snapshot.
    pub(crate) async fn build_agent_context(
        &self,
        record: &SessionRecord,
        identity: AgentTurnRef,
    ) -> TalosResult<AgentContext> {
        if record.id != identity.session_id {
            return Err(TalosError::internal(
                "Agent identity does not match its source Session",
            ));
        }
        if record.agent_id.trim().is_empty() {
            return Err(
                TalosError::internal("Session is missing its Agent binding.").context(format!(
                    "session '{}' is missing its Agent binding",
                    record.id
                )),
            );
        }
        let model_selection = SessionConfigOverridePolicy::extract_model_selection(&record.conf)?
            .ok_or_else(|| {
            TalosError::internal("Session is missing its model selection.").context(format!(
                "session '{}' is missing its model selection",
                record.id
            ))
        })?;
        let model = self.model_info_for_selection(&model_selection)?;
        let session_context = self.build_agent_session_context(record)?;
        Ok(AgentContext {
            session: session_context,
            turn_id: identity.turn_id,
            project_id: record.project_id.clone(),
            working_directory: self.resolve_effective_root(record).await?,
            environment: record.environment.clone(),
            model,
            model_selection,
            permission_mode: SessionConfigOverridePolicy::resolve_user_permission_mode(
                &self.base_config_snapshot(),
                &record.conf,
            ),
        })
    }

    /// Build the minimal Provider Session-management input from product owners.
    pub(crate) fn build_agent_session_context(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<AgentSessionContext> {
        if record.agent_id.trim().is_empty() {
            return Err(
                TalosError::internal("Session is missing its Agent binding.").context(format!(
                    "session '{}' is missing its Agent binding",
                    record.id
                )),
            );
        }
        Ok(AgentSessionContext {
            agent_id: record.agent_id.clone(),
            session_id: record.id.clone(),
            workspace: self
                .base_config_snapshot()
                .resolve_workspace(&record.workspace_id, &record.user_id)?,
        })
    }

    pub(crate) async fn load_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
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

    pub(crate) async fn patch_session(
        &self,
        record: SessionRecord,
        patch: SessionPatch,
    ) -> TalosResult<Option<SessionRecord>> {
        self.validate_persisted_model_selection(&record)?;
        let patch = self.normalize_session_patch(record.environment.as_ref(), patch)?;
        self.apply_session_patch(record, patch).await
    }

    pub(crate) async fn clear_session_history(
        &self,
        mut record: SessionRecord,
    ) -> TalosResult<SessionRecord> {
        let cleared_at = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        if !self
            .session_store
            .clear_session_history(&record.id, cleared_at)
            .await?
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                format!("Session {} was not found.", record.id),
            ));
        }
        record.title = DEFAULT_SESSION_TITLE.to_string();
        record.last_activity = cleared_at;
        record.message_count = 0;
        record.latest_turn_seq = 0;
        record.memory_processed_until_event_seq = 0;
        record.usage = talos_types::SessionUsage::default();
        Ok(record)
    }

    pub(crate) async fn record_compaction_usage(
        &self,
        record: SessionRecord,
        usage: &talos_types::talos::TokenUsage,
    ) -> TalosResult<SessionRecord> {
        self.validate_persisted_model_selection(&record)?;
        let last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        let session_id = record.id;
        self.session_store
            .accumulate_session_usage(AccumulateSessionUsageParams {
                session_id: session_id.clone(),
                tokens: usage.clone(),
                last_activity,
            })
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("Session {session_id} was not found."),
                )
            })
    }

    pub(crate) async fn delete_session(&self, record: SessionRecord) -> TalosResult<bool> {
        let session_id = &record.id;
        if matches!(record.tree_identity, SessionTreeIdentity::Subagent { .. }) {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("session '{session_id}' is controlled by its parent agent"),
            ));
        }
        self.session_store.delete_session_tree(session_id).await
    }

    pub(crate) async fn delete_subagent_session(&self, record: SessionRecord) -> TalosResult<bool> {
        let session_id = &record.id;
        if !matches!(record.tree_identity, SessionTreeIdentity::Subagent { .. }) {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("session '{session_id}' is not a subagent session"),
            ));
        }
        self.session_store.delete_session_record(session_id).await
    }

    fn normalize_session_patch(
        &self,
        environment: Option<&talos_types::talos::TalosSessionEnvironment>,
        mut patch: SessionPatch,
    ) -> TalosResult<SessionPatch> {
        let connection_mode = environment
            .map(|environment| environment.connection_mode)
            .unwrap_or_default();
        if let Some(OptionalCurrentDirectoryPatch::Set { path }) = patch.current_directory.as_ref()
        {
            patch.current_directory =
                normalize_current_directory(connection_mode, Some(path.clone()))?
                    .map(|normalized| OptionalCurrentDirectoryPatch::Set { path: normalized });
        }
        if matches!(connection_mode, TalosConnectionMode::Remote) {
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
            .resolve_workspace(&params.workspace_id, &params.user_id)?;
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
        self.model_info_for_selection(&stored_selection)?;
        let now = chrono::Utc::now().timestamp_millis();
        let source = params.source;
        let record = SessionRecord {
            id: session_id,
            agent_id: params.target_agent,
            execution_node_id: params.execution_node_id,
            tree_identity,
            workspace_id: params.workspace_id,
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: params.user_id,
            source_type: source.source_type,
            source_provider: source.source_provider,
            channel_id: source.channel_id,
            thread_id: source.thread_id,
            environment: source.environment,
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            latest_turn_seq: 0,
            memory_processed_until_event_seq: 0,
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
    ) -> TalosResult<Option<SessionRecord>> {
        let previous_selection =
            SessionConfigOverridePolicy::extract_model_selection(&record.conf)?;
        if let Some(conf) = patch.conf.as_ref() {
            SessionConfigOverridePolicy::validate_conf(conf)?;
        }
        let patch_conf = patch.conf.as_ref().cloned().unwrap_or_default();
        record.conf = SessionConfigOverridePolicy::merge_conf(&record.conf, &patch_conf);
        let next_selection = SessionConfigOverridePolicy::extract_model_selection(&record.conf)?;
        if let Some(selection) = next_selection.as_ref() {
            self.model_info_for_selection(selection)?;
        }
        let reset_context_usage = next_selection != previous_selection;
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
        let last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        self.session_store
            .patch_session(PatchSessionParams {
                session_id: record.id,
                conf: record.conf,
                current_directory: record.current_directory,
                reset_context_usage,
                last_activity,
            })
            .await
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
