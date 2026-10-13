use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use talos_agent::AgentContext;
use talos_agent_api::AgentTurnRef;
use talos_error::{TalosError, TalosResult};
use talos_store::project::ProjectStore;
use talos_store::session::{
    AccumulateSessionUsageParams, PatchSessionParams, SessionQuery, SessionRecord, SessionStore,
    SessionTreeIdentity, SessionTreeScope,
};
use talos_types::{
    OptionalCurrentDirectoryPatch, SessionConfigOverridePolicy, SessionInfo, SessionPatch,
    TalosConfig, UserPermissionMode, DEFAULT_SESSION_TITLE,
};

use super::CreateSessionParams;

#[path = "environment.rs"]
mod environment;

#[path = "title.rs"]
mod title;

#[path = "execution_boundary.rs"]
mod execution_boundary;

#[derive(Clone)]
pub(crate) struct SessionService {
    session_store: Arc<dyn SessionStore>,
    project_store: Arc<dyn ProjectStore>,
    base_config: Arc<RwLock<TalosConfig>>,
    coordinator: Arc<dyn talos_coordinator::AgentCoordinator>,
    sync: Arc<RwLock<execution_boundary::SessionSyncState>>,
}

impl SessionService {
    pub(crate) async fn claim_local_history(&self, user_id: &str) -> TalosResult<()> {
        let records = self
            .session_store
            .list_session_records(&talos_store::session::SessionQuery {
                user_id: Some("anonymous".into()),
                execution_node_id: Some("embedded".into()),
                ..Default::default()
            })
            .await?;
        for record in records {
            self.ensure_worker_idle(&record).await?;
        }
        self.session_store.claim_local_history(user_id).await
    }

    pub(crate) async fn reject_nonterminal_turns(&self, node_id: &str) -> TalosResult<()> {
        if self
            .session_store
            .has_nonterminal_turns(Some(node_id))
            .await?
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Computer configuration cannot change while an Agent Turn is non-terminal",
            ));
        }
        Ok(())
    }

    pub fn new(
        session_store: Arc<dyn SessionStore>,
        project_store: Arc<dyn ProjectStore>,
        base_config: TalosConfig,
        coordinator: Arc<dyn talos_coordinator::AgentCoordinator>,
    ) -> Self {
        Self {
            session_store,
            project_store,
            base_config: Arc::new(RwLock::new(base_config)),
            coordinator,
            sync: Default::default(),
        }
    }

    async fn validate_persisted_model_selection(&self, record: &SessionRecord) -> TalosResult<()> {
        if record.conf.model_selection.is_none()
            && self
                .session_store
                .get_shared_session_info(&record.id)
                .await?
                .is_some()
        {
            return SessionConfigOverridePolicy::validate_conf(&record.conf);
        }
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

    pub(crate) fn kernel_timezone(&self) -> String {
        self.base_config_snapshot().agent.kernel.timezone
    }

    pub(crate) async fn session_info(&self, record: &SessionRecord) -> TalosResult<SessionInfo> {
        let base_config = self.base_config_snapshot();
        let shared = self
            .session_store
            .get_shared_session_info(&record.id)
            .await?;
        let model_selection = record
            .conf
            .model_selection
            .clone()
            .or_else(|| shared.as_ref().map(|info| info.model_selection.clone()))
            .ok_or_else(|| TalosError::internal("Session is missing its model selection."))?;
        let configured_default_mode = shared.as_ref().map_or_else(
            || {
                UserPermissionMode::from(
                    &base_config
                        .agent
                        .kernel
                        .tools
                        .sandbox_policy
                        .user_permission_mode,
                )
            },
            |info| info.configured_default_mode,
        );
        let effective_mode = record
            .conf
            .user_permission_mode
            .unwrap_or(configured_default_mode);
        Ok(SessionInfo {
            node_id: record.execution_node_id.clone(),
            id: record.id.clone(),
            title: record.title.clone(),
            user_id: record.user_id.clone(),
            source_type: record.source_type.clone(),
            source_provider: record.source_provider.clone(),
            model_selection,
            configured_default_mode,
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
        if record.current_directory.is_some() {
            return Ok(record.current_directory.clone());
        }
        // The execution node resolves the implicit Workspace from its own configuration.
        Ok(None)
    }

    async fn validate_project_binding(
        &self,
        project_id: &str,
        user_id: &str,
        node_id: &str,
    ) -> TalosResult<()> {
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
        if project.execution_node_id != node_id {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Session and Project Worker bindings differ",
            ));
        }
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
            self.validate_project_binding(project_id, &params.user_id, &params.execution_node_id)
                .await?;
        }
        params.current_directory = self
            .normalize_current_directory(
                &params.execution_node_id,
                &params.user_id,
                params.current_directory,
            )
            .await?;
        let record = self.build_session_record(params, tree_identity).await?;
        self.session_store.insert_session_record(&record).await?;
        Ok(record)
    }

    pub(crate) async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<SessionInfo>> {
        let records = self.session_store.list_session_records(&query).await?;
        let mut sessions = Vec::with_capacity(records.len());
        for record in &records {
            self.validate_persisted_model_selection(record).await?;
            sessions.push(self.session_info(record).await?);
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
        let prepared = self.prepare_execution(record).await?;
        let record = &prepared;
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
        let info = self.session_info(record).await?;
        let model_selection = info.model_selection;
        let model = self
            .model_info_for_selection(&record.execution_node_id, &record.user_id, &model_selection)
            .await?;
        let session_context = self.build_agent_session_context(record).await?;
        Ok(AgentContext {
            history_cursor: self.session_store.get_history_cursor(&record.id).await?,
            session: session_context,
            turn_id: identity.turn_id,
            project_id: record.project_id.clone(),
            working_directory: self.resolve_effective_root(record).await?,
            environment: record.environment.clone(),
            model,
            model_selection,
            permission_mode: info.effective_mode,
        })
    }

    pub(crate) async fn commit_worker_history_cursor(
        &self,
        identity: &AgentTurnRef,
        visibility: agent_protocol::SessionStateVisibility,
    ) -> TalosResult<()> {
        let Some(history_cursor) = self
            .session_store
            .get_history_cursor(&identity.session_id)
            .await?
        else {
            return Ok(());
        };
        if history_cursor.completed_turn_id.as_deref() != Some(identity.turn_id.as_str()) {
            return Ok(());
        }
        let record = self
            .load_record(&identity.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::SessionNotFound,
                    "Session was not found",
                )
            })?;
        let session = self.build_agent_session_context(&record).await?;
        self.coordinator
            .commit_session_execution_state(agent_protocol::CommitSessionExecutionStateParams {
                session,
                identity: Some(identity.clone()),
                history_cursor,
                visibility,
            })
            .await
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
        self.validate_persisted_model_selection(&record).await?;
        let patch = self.normalize_session_patch(&record, patch).await?;
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
        self.validate_persisted_model_selection(&record).await?;
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

    async fn normalize_session_patch(
        &self,
        record: &SessionRecord,
        mut patch: SessionPatch,
    ) -> TalosResult<SessionPatch> {
        if let Some(OptionalCurrentDirectoryPatch::Set { path }) = patch.current_directory.as_ref()
        {
            patch.current_directory = self
                .normalize_current_directory(
                    &record.execution_node_id,
                    &record.user_id,
                    Some(path.clone()),
                )
                .await?
                .map(|path| OptionalCurrentDirectoryPatch::Set { path });
        }
        Ok(patch)
    }

    async fn build_session_record(
        &self,
        params: CreateSessionParams,
        tree_identity: SessionTreeIdentity,
    ) -> TalosResult<SessionRecord> {
        self.worker_environment(&params.execution_node_id, &params.user_id)
            .await?
            .workspace_info(Self::environment_context(
                &params.user_id,
                Some(params.workspace_id.clone()),
            ))
            .await?;
        let session_id = params
            .session_id
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let requested_selection = params.model_selection;
        let effective_selection = match requested_selection {
            Some(selection) => selection,
            None => {
                self.worker_environment(&params.execution_node_id, &params.user_id)
                    .await?
                    .list_models(Self::environment_context(&params.user_id, None))
                    .await?
                    .default_model_selection
            }
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
        self.model_info_for_selection(
            &params.execution_node_id,
            &params.user_id,
            &stored_selection,
        )
        .await?;
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
            self.model_info_for_selection(&record.execution_node_id, &record.user_id, selection)
                .await?;
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

#[cfg(test)]
#[path = "worker_binding_tests.rs"]
mod worker_binding_tests;
