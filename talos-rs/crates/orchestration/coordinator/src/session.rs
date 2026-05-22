use crate::CoordinatorImpl;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use talos_config::{provider_config_for_llm, resolve_runtime_path, LlmConfig};
use talos_error::{TalosError, TalosResult};
use talos_session_store::SessionStore;
use talos_types::gateway::GatewayRequest;
use talos_types::talos::{TalosConnectionMode, TalosContext, TalosProvider, TalosUsage};
use talos_types::{
    connection_mode_from_context, normalize_current_directory, resolve_effective_project_root,
    validate_project_directory, OptionalCurrentDirectoryPatch, OptionalProjectIdPatch,
    ProjectPatch, ProjectRecord, SessionConf, SessionConfigOverridePolicy, SessionPatch,
    SessionPatchAck, SessionQuery, SessionRecord, TalosConfig, TalosSession,
};

#[derive(Clone)]
pub(super) struct SessionManager {
    session_store: Arc<dyn SessionStore>,
    session_cache: Arc<RwLock<HashMap<String, TalosSession>>>,
    base_config: Arc<RwLock<TalosConfig>>,
    default_workspace_id: String,
    channel_dm_scopes: HashMap<String, String>,
}

impl SessionManager {
    pub(super) fn new(session_store: Arc<dyn SessionStore>, base_config: TalosConfig) -> Self {
        Self {
            session_store,
            session_cache: Arc::new(RwLock::new(HashMap::new())),
            base_config: Arc::new(RwLock::new(base_config)),
            default_workspace_id: "default".to_string(),
            channel_dm_scopes: HashMap::new(),
        }
    }

    pub(super) fn with_default_workspace_id(mut self, workspace_id: impl Into<String>) -> Self {
        self.default_workspace_id = workspace_id.into();
        self
    }

    pub(super) fn with_channel_dm_scope(
        mut self,
        channel_id: impl Into<String>,
        dm_scope: impl Into<String>,
    ) -> Self {
        self.channel_dm_scopes
            .insert(channel_id.into(), dm_scope.into());
        self
    }

    pub(super) fn default_workspace_id(&self) -> &str {
        self.default_workspace_id.as_str()
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

    pub(super) fn update_base_llm(&self, llm: &LlmConfig) {
        let mut base_config = self
            .base_config
            .write()
            .expect("session base config write lock");
        base_config.llm = llm.clone();
        base_config.agent.model = llm.model.clone();
        self.session_cache
            .write()
            .expect("session cache write lock")
            .clear();
    }

    fn resolve_dm_scope(&self, provider: &TalosProvider) -> Option<String> {
        self.channel_dm_scopes.get(provider.as_str()).cloned()
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

    fn workspace_root(&self) -> Option<PathBuf> {
        let base_config = self.base_config_snapshot();
        let workspace_dir = base_config.agent.kernel.memory.workspace_dir.trim();
        if workspace_dir.is_empty() {
            return None;
        }

        let candidate = resolve_runtime_path(workspace_dir);
        candidate.exists().then_some(candidate)
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

    async fn cache_session(&self, session: TalosSession) -> TalosSession {
        let session_id = session.context.session_id.clone();
        self.session_cache
            .write()
            .expect("session cache write lock")
            .insert(session_id, session.clone());
        session
    }

    async fn get_cached_session(&self, session_id: &str) -> Option<TalosSession> {
        self.session_cache
            .read()
            .expect("session cache read lock")
            .get(session_id)
            .cloned()
    }

    pub(super) async fn invalidate_cached_session(&self, session_id: &str) {
        self.session_cache
            .write()
            .expect("session cache write lock")
            .remove(session_id);
    }

    async fn update_cached_session_title(&self, session_id: &str, title: &str) {
        let mut cache = self
            .session_cache
            .write()
            .expect("session cache write lock");
        if let Some(session) = cache.get_mut(session_id) {
            session.title = title.to_string();
        }
    }

    pub(super) async fn resolve_session(
        &self,
        request: &GatewayRequest,
    ) -> TalosResult<TalosSession> {
        let workspace_id = request
            .workspace_id
            .clone()
            .unwrap_or_else(|| self.default_workspace_id.clone());
        let session_id = request.session_id.as_str();
        if let Some(session) = self.get_cached_session(session_id).await {
            return Ok(session);
        }

        let record = self.session_store.get_session_record(session_id).await?;
        let record = record
            .ok_or_else(|| TalosError::ResourceNotFound(format!("session {}", session_id)))?;
        let mut session = self.build_session(&record, Some(workspace_id))?;
        session.context.project_root = self.resolve_effective_root(&session).await?;
        Ok(self.cache_session(session).await)
    }

    pub(super) async fn record_turn_completion(
        &self,
        session_id: &str,
        assistant_model: &str,
        message_delta: u64,
        usage: &TalosUsage,
    ) -> TalosResult<()> {
        self.session_store
            .record_session_progress(session_id, message_delta, usage, Some(assistant_model))
            .await?;
        self.update_cached_session_after_turn(session_id, assistant_model, message_delta, usage)
            .await;
        Ok(())
    }

    pub(super) async fn record_session_usage(
        &self,
        session_id: &str,
        assistant_model: &str,
        usage: &TalosUsage,
    ) -> TalosResult<()> {
        self.record_turn_completion(session_id, assistant_model, 0, usage)
            .await
    }

    pub(super) async fn record_stream_completion(
        &self,
        session_id: &str,
        assistant_model: &str,
        message_delta: u64,
        usage: &TalosUsage,
    ) {
        let _ = self
            .session_store
            .record_session_progress(session_id, message_delta, usage, Some(assistant_model))
            .await;
        self.update_cached_session_after_turn(session_id, assistant_model, message_delta, usage)
            .await;
    }

    async fn update_cached_session_after_turn(
        &self,
        session_id: &str,
        assistant_model: &str,
        message_delta: u64,
        usage: &TalosUsage,
    ) {
        let mut cache = self
            .session_cache
            .write()
            .expect("session cache write lock");
        if let Some(session) = cache.get_mut(session_id) {
            session.apply_turn_progress(assistant_model, message_delta, usage);
        }
    }

    pub(super) async fn create_session(
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
                &self.base_model(),
                model.as_deref(),
                conf,
                self.resolve_dm_scope(&context.provider),
                project_id,
                current_directory,
            )
            .await?;
        let mut session = self.build_session(&record, Some(self.default_workspace_id.clone()))?;
        session.context.project_root = self.resolve_effective_root(&session).await?;
        Ok(self.cache_session(session).await)
    }

    pub(super) async fn list_sessions(
        &self,
        query: SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
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

    pub(super) async fn count_sessions(&self, query: SessionQuery) -> TalosResult<usize> {
        self.session_store.count_session_records(&query).await
    }

    pub(super) async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        if let Some(session) = self.get_cached_session(session_id).await {
            return Ok(Some(session));
        }
        self.load_session(session_id, Some(self.default_workspace_id.clone()))
            .await
    }

    pub(super) async fn load_session(
        &self,
        session_id: &str,
        workspace_id: Option<String>,
    ) -> TalosResult<Option<TalosSession>> {
        if let Some(session) = self.get_cached_session(session_id).await {
            return Ok(Some(session));
        }
        let Some(record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(None);
        };
        let mut session = self.build_session(
            &record,
            workspace_id.or_else(|| Some(self.default_workspace_id.clone())),
        )?;
        session.context.project_root = self.resolve_effective_root(&session).await?;
        Ok(Some(self.cache_session(session).await))
    }

    pub(super) async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let Some(record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };
        let patch = self
            .normalize_session_patch(record.context.clone(), patch)
            .await?;
        if let Some(OptionalProjectIdPatch::Set { project_id }) = patch.project_id.as_ref() {
            self.validate_project_id(project_id).await?;
        }
        let ack = self
            .session_store
            .patch_session_record(session_id, &patch)
            .await?;
        if ack.success {
            self.invalidate_cached_session(session_id).await;
        }
        Ok(ack)
    }

    pub(super) async fn reset_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let ack = self.session_store.reset_session_record(session_id).await?;
        if ack.success {
            self.invalidate_cached_session(session_id).await;
        }
        Ok(ack)
    }

    pub(super) async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let ack = self.session_store.delete_session_record(session_id).await?;
        if ack.success {
            self.invalidate_cached_session(session_id).await;
        }
        Ok(ack)
    }

    async fn normalize_session_patch(
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

    pub(super) async fn update_session_title(
        &self,
        session_id: &str,
        title: &str,
    ) -> TalosResult<()> {
        self.session_store
            .update_session_title(session_id, title)
            .await?;
        self.update_cached_session_title(session_id, title).await;
        Ok(())
    }

    pub(super) async fn load_session_for_close(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<TalosSession>> {
        let session = self
            .load_session(session_id, Some(self.default_workspace_id.clone()))
            .await?;
        Ok(session)
    }

    pub(super) async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        self.session_store.list_projects().await
    }

    pub(super) async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        self.session_store.get_project(project_id).await
    }

    pub(super) async fn create_project(
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

    pub(super) async fn patch_project(
        &self,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        self.session_store.patch_project(project_id, &patch).await
    }

    pub(super) async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
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

impl CoordinatorImpl {
    pub fn update_base_llm(&self, llm: &LlmConfig) {
        self.sessions.update_base_llm(llm);
    }

    pub fn with_default_workspace_id(mut self, workspace_id: impl Into<String>) -> Self {
        self.sessions = Arc::new(
            self.sessions
                .as_ref()
                .clone()
                .with_default_workspace_id(workspace_id),
        );
        self
    }

    pub fn with_channel_dm_scope(
        mut self,
        channel_id: impl Into<String>,
        dm_scope: impl Into<String>,
    ) -> Self {
        self.sessions = Arc::new(
            self.sessions
                .as_ref()
                .clone()
                .with_channel_dm_scope(channel_id, dm_scope),
        );
        self
    }
}

#[cfg(test)]
mod tests {
    use super::SessionManager;
    use std::sync::Arc;
    use talos_session_store::MockSessionStore;
    use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};

    fn provider_model(id: &str) -> talos_config::ProviderModelConfig {
        talos_config::ProviderModelConfig {
            id: id.to_string(),
            name: id.to_string(),
            context_window: 32_768,
            reasoning: false,
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
        let manager = SessionManager::new(Arc::new(MockSessionStore::new()), base_config);
        let mut llm = talos_config::LlmConfig::default();
        llm.provider = "openai".to_string();
        llm.model = "new-model".to_string();

        manager.update_base_llm(&llm);
        let session = manager
            .create_session(context(), "default", None, None, None, None)
            .await
            .expect("session should be created");

        assert_eq!(session.config.llm.provider, "openai");
        assert_eq!(session.config.llm.model, "new-model");
    }

    #[tokio::test]
    async fn provider_switch_clears_cached_session_and_resets_model_to_new_default() {
        let mut base_config = talos_types::TalosConfig::default();
        base_config.llm.provider = "anthropic".to_string();
        base_config.llm.model = "old-default".to_string();
        base_config.llm.anthropic.models = vec![
            provider_model("old-default"),
            provider_model("session-model"),
        ];
        let manager = SessionManager::new(Arc::new(MockSessionStore::new()), base_config);
        let original = manager
            .create_session(
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
        manager.update_base_llm(&llm);

        let reloaded = manager
            .get_session(&original.context.session_id)
            .await
            .expect("session read should succeed")
            .expect("session should exist");

        assert_eq!(reloaded.config.llm.provider, "openai");
        assert_eq!(reloaded.config.llm.model, "new-default");
    }

    #[tokio::test]
    async fn requested_session_model_wins_when_current_provider_supports_it() {
        let mut base_config = talos_types::TalosConfig::default();
        base_config.llm.provider = "anthropic".to_string();
        base_config.llm.model = "old-default".to_string();
        base_config.llm.anthropic.models = vec![provider_model("old-default")];
        let manager = SessionManager::new(Arc::new(MockSessionStore::new()), base_config);
        let mut llm = talos_config::LlmConfig::default();
        llm.provider = "anthropic".to_string();
        llm.model = "new-default".to_string();
        llm.anthropic.models = vec![
            provider_model("new-default"),
            provider_model("session-model"),
        ];

        manager.update_base_llm(&llm);
        let session = manager
            .create_session(
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
}
