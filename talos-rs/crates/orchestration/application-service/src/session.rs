use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use talos_config::{provider_config_for_llm, resolve_runtime_path, LlmConfig};
use talos_error::{TalosError, TalosResult};
use talos_session_store::SessionStore;
use talos_types::talos::{TalosConnectionMode, TalosContext, TalosProvider};
use talos_types::{
    connection_mode_from_context, normalize_current_directory, resolve_effective_project_root,
    validate_project_directory, OptionalCurrentDirectoryPatch, OptionalProjectIdPatch,
    ProjectPatch, ProjectRecord, SessionConf, SessionConfigOverridePolicy, SessionPatch,
    SessionPatchAck, SessionQuery, SessionRecord, TalosConfig, TalosSession,
};

#[derive(Clone)]
pub struct SessionService {
    session_store: Arc<dyn SessionStore>,
    base_config: Arc<RwLock<TalosConfig>>,
    default_workspace_id: String,
    channel_dm_scopes: std::collections::HashMap<String, String>,
}

impl SessionService {
    pub fn new(session_store: Arc<dyn SessionStore>, base_config: TalosConfig) -> Self {
        Self {
            session_store,
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
        record: &SessionRecord,
        workspace_id: Option<String>,
    ) -> TalosResult<TalosSession> {
        let base_config = self.base_config_snapshot();
        let record = self.normalize_record_for_base_provider(record, &base_config)?;
        let mut session = TalosSession::restore(&record, &base_config, workspace_id)?;
        session.context.project_root = None;
        Ok(session)
    }

    fn normalize_record_for_base_provider(
        &self,
        record: &SessionRecord,
        base_config: &TalosConfig,
    ) -> TalosResult<SessionRecord> {
        let explicit_model = SessionConfigOverridePolicy::extract_agent_model(&record.conf)?;
        let effective_model = explicit_model.as_deref().unwrap_or(record.model.as_str());
        if self.base_provider_supports_model(base_config, effective_model) {
            return Ok(record.clone());
        }

        let mut normalized = record.clone();
        normalized.model = base_config.llm.model.clone();
        normalized.conf = clear_session_model_override(&normalized.conf);
        Ok(normalized)
    }

    fn base_provider_supports_model(&self, base_config: &TalosConfig, model: &str) -> bool {
        if model == base_config.llm.model {
            return true;
        }
        let Some(provider) = provider_config_for_llm(&base_config.llm, &base_config.llm.provider)
        else {
            return false;
        };
        provider.models.is_empty() || provider.models.iter().any(|entry| entry.id == model)
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
        session_id: Option<&str>,
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
        let base_model = self.base_model();
        let record = self
            .session_store
            .create_session(
                session_id,
                &context,
                target_agent,
                &base_model,
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

fn clear_session_model_override(conf: &SessionConf) -> SessionConf {
    let mut next = conf.clone();
    if let Some(agent) = next.agent.as_mut() {
        agent.model = None;
    }
    if next
        .agent
        .as_ref()
        .is_some_and(|agent| agent.model.is_none())
    {
        next.agent = None;
    }
    next
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use talos_session_store::MockSessionStore;
    use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};

    use super::SessionService;

    fn provider_model(id: &str) -> talos_config::ProviderModelConfig {
        talos_config::ProviderModelConfig {
            id: id.to_string(),
            name: id.to_string(),
            context_window: 32_768,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        }
    }

    fn context() -> TalosContext {
        TalosContext {
            source_type: TalosSourceType::System,
            provider: TalosProvider::Http,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            project_root: None,
            environment: None,
        }
    }

    #[tokio::test]
    async fn update_base_llm_changes_new_session_default_model() {
        let mut base_config = talos_types::TalosConfig::default();
        base_config.llm.provider = "anthropic".to_string();
        base_config.llm.model = "old-model".to_string();
        let service = SessionService::new(Arc::new(MockSessionStore::new()), base_config);
        let mut llm = talos_config::LlmConfig::default();
        llm.provider = "openai".to_string();
        llm.model = "new-model".to_string();

        service.update_base_llm(&llm);
        let session = service
            .create_session(None, context(), "default", None, None, None, None)
            .await
            .expect("session should be created");

        assert_eq!(session.config.llm.provider, "openai");
        assert_eq!(session.config.llm.model, "new-model");
    }

    #[tokio::test]
    async fn requested_session_model_wins_when_provider_is_unchanged() {
        let mut base_config = talos_types::TalosConfig::default();
        base_config.llm.provider = "anthropic".to_string();
        base_config.llm.model = "old-default".to_string();
        base_config.llm.anthropic.models = vec![provider_model("old-default")];
        let service = SessionService::new(Arc::new(MockSessionStore::new()), base_config);
        let mut llm = talos_config::LlmConfig::default();
        llm.provider = "anthropic".to_string();
        llm.model = "new-default".to_string();
        llm.anthropic.models = vec![
            provider_model("new-default"),
            provider_model("session-model"),
        ];

        service.update_base_llm(&llm);
        let session = service
            .create_session(
                None,
                context(),
                "default",
                Some("session-model".to_string()),
                None,
                None,
                None,
            )
            .await
            .expect("session should be created");

        assert_eq!(session.config.llm.provider, "anthropic");
        assert_eq!(session.config.llm.model, "session-model");
    }

    #[tokio::test]
    async fn provider_switch_resets_existing_session_model_to_new_default() {
        let mut base_config = talos_types::TalosConfig::default();
        base_config.llm.provider = "anthropic".to_string();
        base_config.llm.model = "old-default".to_string();
        base_config.llm.anthropic.models = vec![
            provider_model("old-default"),
            provider_model("session-model"),
        ];
        let service = SessionService::new(Arc::new(MockSessionStore::new()), base_config);

        let original = service
            .create_session(
                None,
                context(),
                "default",
                Some("session-model".to_string()),
                None,
                None,
                None,
            )
            .await
            .expect("session should be created");
        assert_eq!(original.config.llm.model, "session-model");

        let mut llm = talos_config::LlmConfig::default();
        llm.provider = "openai".to_string();
        llm.model = "new-default".to_string();
        llm.openai.models = vec![provider_model("new-default")];
        service.update_base_llm(&llm);

        let reloaded = service
            .get_session(&original.context.session_id)
            .await
            .expect("session read should succeed")
            .expect("session should exist");

        assert_eq!(reloaded.config.llm.provider, "openai");
        assert_eq!(reloaded.config.llm.model, "new-default");
    }
}
