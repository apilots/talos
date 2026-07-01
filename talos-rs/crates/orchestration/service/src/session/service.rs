use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use talos_config::{resolve_runtime_path, LlmConfig};
use talos_error::{TalosError, TalosResult};
use talos_store::project::ProjectStore;
use talos_store::session::{SessionPatchAck, SessionQuery, SessionStore};
use talos_types::talos::{TalosConnectionMode, TalosContext, TalosProvider};
use talos_types::{
    connection_mode_from_context, normalize_current_directory, resolve_effective_project_root,
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, SessionConf, SessionPatch, TalosConfig,
    TalosSession,
};

use crate::session::{model_projection, SessionRecordService};

#[derive(Clone)]
pub struct SessionService {
    session_records: SessionRecordService,
    session_store: Arc<dyn SessionStore>,
    project_store: Arc<dyn ProjectStore>,
    base_config: Arc<RwLock<TalosConfig>>,
    default_workspace_id: String,
    channel_dm_scopes: std::collections::HashMap<String, String>,
}

impl SessionService {
    pub fn new(
        session_store: Arc<dyn SessionStore>,
        project_store: Arc<dyn ProjectStore>,
        base_config: TalosConfig,
    ) -> Self {
        Self {
            session_records: SessionRecordService::new(session_store.clone()),
            session_store,
            project_store,
            base_config: Arc::new(RwLock::new(base_config)),
            default_workspace_id: "default".to_string(),
            channel_dm_scopes: std::collections::HashMap::new(),
        }
    }

    pub fn with_default_workspace_id(mut self, workspace_id: impl Into<String>) -> Self {
        self.default_workspace_id = workspace_id.into();
        self
    }

    pub fn with_channel_dm_scope(
        mut self,
        channel_id: impl Into<String>,
        dm_scope: impl Into<String>,
    ) -> Self {
        self.channel_dm_scopes
            .insert(channel_id.into(), dm_scope.into());
        self
    }

    fn base_model(&self) -> String {
        self.base_config_snapshot().llm.model
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
        base_config.agent.model = llm.model.clone();
    }

    fn resolve_dm_scope(&self, provider: &TalosProvider) -> Option<String> {
        self.channel_dm_scopes.get(provider.as_str()).cloned()
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        let base_config = self.base_config_snapshot();
        let workspace_dir = base_config.agent.kernel.workspace_dir.trim();
        if workspace_dir.is_empty() {
            return None;
        }

        let candidate = resolve_runtime_path(workspace_dir);
        candidate.exists().then_some(candidate)
    }

    fn build_session(
        &self,
        record: &talos_store::session::SessionRecord,
        workspace_id: Option<String>,
    ) -> TalosResult<TalosSession> {
        model_projection::build_session(record, workspace_id, &self.base_config_snapshot())
    }

    async fn resolve_effective_root(&self, session: &TalosSession) -> TalosResult<Option<PathBuf>> {
        let project_directory = if let Some(project_id) = session.project_id.as_deref() {
            Some(
                self.project_store
                    .get_project(project_id)
                    .await?
                    .ok_or_else(|| TalosError::ResourceNotFound(format!("project {}", project_id)))?
                    .directory,
            )
        } else {
            None
        };
        Ok(resolve_effective_project_root(
            session,
            project_directory,
            self.workspace_root(),
        ))
    }

    async fn validate_project_id(&self, project_id: &str) -> TalosResult<()> {
        let exists = self.project_store.get_project(project_id).await?.is_some();
        if !exists {
            return Err(TalosError::ValidationError(format!(
                "project {} not found",
                project_id
            )));
        }
        Ok(())
    }

    pub async fn create_session(
        &self,
        session_id: Option<&str>,
        context: TalosContext,
        target_agent: &str,
        model: Option<String>,
        conf: Option<SessionConf>,
        workspace_id: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<TalosSession> {
        if let Some(project_id) = project_id.as_deref() {
            self.validate_project_id(project_id).await?;
        }
        let mut context = context;
        context.project_root = None;
        let current_directory = normalize_current_directory(&context, current_directory)?;
        let base_model = self.base_model();
        let record = self
            .session_records
            .create_session_record(
                session_id,
                &context,
                target_agent,
                &base_model,
                model.as_deref(),
                conf,
                self.resolve_dm_scope(&context.provider),
                workspace_id,
                project_id,
                current_directory,
            )
            .await?;
        let mut session = self.build_session(&record, Some(self.default_workspace_id.clone()))?;
        session.context.project_root = self.resolve_effective_root(&session).await?;
        Ok(session)
    }

    pub async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
        let records = self.session_store.list_session_records(&query).await?;
        let mut sessions = Vec::with_capacity(records.len());
        for record in &records {
            let mut session =
                self.build_session(record, Some(self.default_workspace_id.clone()))?;
            session.context.project_root = self.resolve_effective_root(&session).await?;
            sessions.push(session);
        }
        Ok(sessions)
    }

    pub async fn count_sessions(&self, query: SessionQuery) -> TalosResult<usize> {
        self.session_store.count_session_records(&query).await
    }

    pub async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        self.load_session(session_id, Some(self.default_workspace_id.clone()))
            .await
    }

    pub async fn load_session(
        &self,
        session_id: &str,
        workspace_id: Option<String>,
    ) -> TalosResult<Option<TalosSession>> {
        let Some(record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(None);
        };
        let mut session = self.build_session(
            &record,
            workspace_id.or_else(|| Some(self.default_workspace_id.clone())),
        )?;
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
        let patch = self.normalize_session_patch(record.context.clone(), patch)?;
        if let Some(OptionalProjectIdPatch::Set { project_id }) = patch.project_id.as_ref() {
            self.validate_project_id(project_id).await?;
        }
        let ack = self
            .session_records
            .patch_session_record(session_id, &patch)
            .await?;
        Ok(ack)
    }

    pub async fn reset_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.session_records.reset_session_record(session_id).await
    }

    pub async fn record_session_usage(
        &self,
        session_id: &str,
        assistant_model: &str,
        usage: &talos_types::talos::TokenUsage,
    ) -> TalosResult<()> {
        self.session_records
            .record_session_progress(session_id, 0, usage, Some(assistant_model))
            .await
    }

    pub async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
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
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
