use std::path::PathBuf;
use std::sync::Arc;

use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use talos_session_store::SessionStore;
use talos_types::talos::{TalosConnectionMode, TalosContext, TalosProvider};
use talos_types::{
    connection_mode_from_context, normalize_current_directory, resolve_effective_project_root,
    validate_project_directory, OptionalCurrentDirectoryPatch, OptionalProjectIdPatch,
    ProjectPatch, ProjectRecord, SessionConf, SessionPatch, SessionPatchAck, SessionQuery,
    SessionRecord, TalosConfig, TalosSession,
};

#[derive(Clone)]
pub struct SessionService {
    session_store: Arc<dyn SessionStore>,
    base_config: TalosConfig,
    default_workspace_id: String,
    channel_dm_scopes: std::collections::HashMap<String, String>,
}

impl SessionService {
    pub fn new(session_store: Arc<dyn SessionStore>, base_config: TalosConfig) -> Self {
        Self {
            session_store,
            base_config,
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

    fn base_model(&self) -> &str {
        self.base_config.llm.model.as_str()
    }

    fn resolve_dm_scope(&self, provider: &TalosProvider) -> Option<String> {
        self.channel_dm_scopes.get(provider.as_str()).cloned()
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        let workspace_dir = self.base_config.agent.kernel.memory.workspace_dir.trim();
        if workspace_dir.is_empty() {
            return None;
        }

        let candidate = resolve_runtime_path(workspace_dir);
        candidate.exists().then_some(candidate)
    }

    fn build_session(
        &self,
        record: &SessionRecord,
        workspace_id: Option<String>,
    ) -> TalosResult<TalosSession> {
        let mut session = TalosSession::restore(record, &self.base_config, workspace_id)?;
        session.context.project_root = None;
        Ok(session)
    }

    async fn resolve_effective_root(&self, session: &TalosSession) -> TalosResult<Option<PathBuf>> {
        let project_directory = if let Some(project_id) = session.project_id.as_deref() {
            Some(
                self.session_store
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
        let exists = self.session_store.get_project(project_id).await?.is_some();
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
        context: TalosContext,
        target_agent: &str,
        model: Option<String>,
        conf: Option<SessionConf>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<TalosSession> {
        if let Some(project_id) = project_id.as_deref() {
            self.validate_project_id(project_id).await?;
        }
        let mut context = context;
        context.project_root = None;
        let current_directory = normalize_current_directory(&context, current_directory)?;
        let record = self
            .session_store
            .create_session(
                &context,
                target_agent,
                self.base_model(),
                model.as_deref(),
                conf,
                self.resolve_dm_scope(&context.provider),
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
            .session_store
            .patch_session_record(session_id, &patch)
            .await?;
        Ok(ack)
    }

    pub async fn reset_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.session_store.reset_session_record(session_id).await
    }

    pub async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.session_store.delete_session_record(session_id).await
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

    pub async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        self.session_store.list_projects().await
    }

    pub async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        self.session_store.get_project(project_id).await
    }

    pub async fn create_project(
        &self,
        name: String,
        directory: PathBuf,
        user_id: String,
    ) -> TalosResult<ProjectRecord> {
        validate_project_directory(&directory)?;
        self.session_store
            .create_project_for_owner(&name, &directory, &user_id)
            .await
    }

    pub async fn patch_project(
        &self,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        self.session_store.patch_project(project_id, &patch).await
    }

    pub async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
        self.session_store.delete_project(project_id).await
    }
}
