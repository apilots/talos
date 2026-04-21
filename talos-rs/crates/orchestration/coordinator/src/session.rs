use crate::CoordinatorImpl;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider};
use talos_session_store::SessionStore;
use talos_types::gateway::GatewayRequest;
use talos_types::talos::{
    TalosConnectionMode, TalosContext, TalosInputItem, TalosProvider, TalosUsage,
};
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, ProjectPatch, ProjectRecord,
    SessionConf, SessionPatch, SessionPatchAck, SessionQuery, SessionRecord, TalosConfig,
    TalosSession, DEFAULT_SESSION_TITLE,
};
use tokio::sync::RwLock;

const SESSION_TITLE_PROMPT: &str = r#"Generate a concise session title for the user's first question.

Rules:
- Return only the title text.
- One line only.
- No markdown, no quotes, no trailing punctuation.
- Prefer 3-8 words.
- Focus on the core task topic.
"#;

#[derive(Clone)]
pub(super) struct SessionTitleGenerator {
    llm_provider: Arc<dyn LlmProvider>,
}

impl SessionTitleGenerator {
    fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    async fn generate(
        &self,
        model: &str,
        first_user_text: &str,
        first_assistant_text: &str,
    ) -> TalosResult<String> {
        let request = ChatRequest::builder()
            .model(model.to_string())
            .messages(vec![
                ChatMessage::system(SESSION_TITLE_PROMPT),
                ChatMessage::user(format!(
                    "First user message:\n{}\n\nFirst assistant response:\n{}",
                    first_user_text.trim(),
                    first_assistant_text.trim()
                )),
            ])
            .build();
        let response = self.llm_provider.chat(request).await?;
        Ok(response.content)
    }
}

fn validate_title(title: &str) -> Option<String> {
    let first_line = title.lines().next().unwrap_or_default().trim();
    let normalized = first_line
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(['.', '!', '?', ':', ';', '。', '！', '？', '：', '；'])
        .trim();
    if normalized.is_empty() || normalized == DEFAULT_SESSION_TITLE {
        return None;
    }
    Some(
        normalized
            .chars()
            .take(48)
            .collect::<String>()
            .trim()
            .to_string(),
    )
    .filter(|value| !value.is_empty())
}

fn deterministic_title(first_user_text: &str) -> String {
    let compact = first_user_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    validate_title(&compact).unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string())
}

fn validate_or_fallback_title(generated: String, first_user_text: &str) -> String {
    validate_title(&generated).unwrap_or_else(|| deterministic_title(first_user_text))
}

pub(super) fn extract_first_user_text(input: &[TalosInputItem]) -> Option<String> {
    input.iter().find_map(|item| match item {
        TalosInputItem::Text { text } => {
            let normalized = text.trim();
            (!normalized.is_empty()).then(|| text.clone())
        }
        _ => None,
    })
}

#[derive(Clone)]
pub(super) struct SessionManager {
    session_store: Arc<dyn SessionStore>,
    session_cache: Arc<RwLock<HashMap<String, TalosSession>>>,
    base_config: TalosConfig,
    default_workspace_id: String,
    channel_dm_scopes: HashMap<String, String>,
    session_title_generator: Option<Arc<SessionTitleGenerator>>,
}

impl SessionManager {
    pub(super) fn new(
        session_store: Arc<dyn SessionStore>,
        base_config: TalosConfig,
        session_title_generator: Option<Arc<SessionTitleGenerator>>,
    ) -> Self {
        Self {
            session_store,
            session_cache: Arc::new(RwLock::new(HashMap::new())),
            base_config,
            default_workspace_id: "default".to_string(),
            channel_dm_scopes: HashMap::new(),
            session_title_generator,
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

    pub(super) fn with_session_title_generator(
        mut self,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> Self {
        self.session_title_generator = Some(Arc::new(SessionTitleGenerator::new(llm_provider)));
        self
    }

    pub(super) fn default_workspace_id(&self) -> &str {
        self.default_workspace_id.as_str()
    }

    fn base_model(&self) -> &str {
        self.base_config.llm.model.as_str()
    }

    fn resolve_dm_scope(&self, provider: &TalosProvider) -> Option<String> {
        self.channel_dm_scopes.get(provider.as_str()).cloned()
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

    fn workspace_root(&self) -> Option<PathBuf> {
        let workspace_dir = self.base_config.agent.kernel.memory.workspace_dir.trim();
        if workspace_dir.is_empty() {
            return None;
        }

        let candidate = resolve_runtime_path(workspace_dir);
        candidate.exists().then_some(candidate)
    }

    fn connection_mode_from_context(&self, context: &TalosContext) -> TalosConnectionMode {
        context
            .environment
            .as_ref()
            .map(|environment| environment.connection_mode)
            .unwrap_or_default()
    }

    fn normalize_current_directory(
        &self,
        context: &TalosContext,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<Option<PathBuf>> {
        match self.connection_mode_from_context(context) {
            TalosConnectionMode::Remote => Ok(None),
            TalosConnectionMode::Embedded => {
                let Some(path) = current_directory else {
                    return Ok(None);
                };
                if !path.is_absolute() {
                    return Err(TalosError::ValidationError(format!(
                        "current directory must be an absolute path: {}",
                        path.display()
                    )));
                }
                if !path.exists() {
                    return Err(TalosError::ValidationError(format!(
                        "current directory does not exist: {}",
                        path.display()
                    )));
                }
                if !path.is_dir() {
                    return Err(TalosError::ValidationError(format!(
                        "current directory is not a directory: {}",
                        path.display()
                    )));
                }
                Ok(Some(path))
            }
        }
    }

    async fn resolve_effective_root(&self, session: &TalosSession) -> TalosResult<Option<PathBuf>> {
        if let Some(project_id) = session.project_id.as_deref() {
            let project = self
                .session_store
                .get_project(project_id)
                .await?
                .ok_or_else(|| TalosError::ResourceNotFound(format!("project {}", project_id)))?;
            return Ok(Some(project.directory));
        }

        if matches!(
            session
                .context
                .environment
                .as_ref()
                .map(|env| env.connection_mode),
            Some(TalosConnectionMode::Embedded)
        ) {
            if let Some(current_directory) = session.current_directory.clone() {
                return Ok(Some(current_directory));
            }
        }

        Ok(self.workspace_root())
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
            .await
            .insert(session_id, session.clone());
        session
    }

    async fn get_cached_session(&self, session_id: &str) -> Option<TalosSession> {
        self.session_cache.read().await.get(session_id).cloned()
    }

    pub(super) async fn invalidate_cached_session(&self, session_id: &str) {
        self.session_cache.write().await.remove(session_id);
    }

    async fn update_cached_session_title(&self, session_id: &str, title: &str) {
        let mut cache = self.session_cache.write().await;
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
        let mut cache = self.session_cache.write().await;
        if let Some(session) = cache.get_mut(session_id) {
            let now = talos_types::now();
            session.state.last_activity = now;
            session.state.message_count = session.state.message_count.saturating_add(message_delta);
            session.state.usage.accumulate(usage);
            session.config.agent.model = assistant_model.to_string();
            session.config.llm.model = assistant_model.to_string();
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
        let current_directory = self.normalize_current_directory(&context, current_directory)?;
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
            patch.current_directory = self
                .normalize_current_directory(&context, Some(path.clone()))?
                .map(|normalized| OptionalCurrentDirectoryPatch::Set { path: normalized });
        }
        if matches!(
            self.connection_mode_from_context(&context),
            TalosConnectionMode::Remote
        ) {
            patch.current_directory = None;
        }
        Ok(patch)
    }

    async fn update_session_title(&self, session_id: &str, title: &str) -> TalosResult<()> {
        self.session_store
            .update_session_title(session_id, title)
            .await?;
        self.update_cached_session_title(session_id, title).await;
        Ok(())
    }

    pub(super) async fn maybe_assign_session_title(
        &self,
        session_id: &str,
        first_user_text: &str,
        first_assistant_text: &str,
    ) -> TalosResult<()> {
        let Some(session) = self.get_session(session_id).await? else {
            return Ok(());
        };
        if session.has_real_title() {
            return Ok(());
        }

        let generated = match self.session_title_generator.as_ref() {
            Some(generator) => generator
                .generate(
                    session.config.llm.model.as_str(),
                    first_user_text,
                    first_assistant_text,
                )
                .await
                .unwrap_or_default(),
            None => String::new(),
        };
        let title = validate_or_fallback_title(generated, first_user_text);
        self.update_session_title(session_id, &title).await
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
        if !directory.is_absolute() {
            return Err(TalosError::ValidationError(format!(
                "project directory must be absolute: {}",
                directory.display()
            )));
        }
        if !directory.exists() {
            return Err(TalosError::ValidationError(format!(
                "project directory does not exist: {}",
                directory.display()
            )));
        }
        if !directory.is_dir() {
            return Err(TalosError::ValidationError(format!(
                "project directory is not a directory: {}",
                directory.display()
            )));
        }
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

impl CoordinatorImpl {
    pub fn with_session_title_generator(mut self, llm_provider: Arc<dyn LlmProvider>) -> Self {
        self.sessions = Arc::new(
            self.sessions
                .as_ref()
                .clone()
                .with_session_title_generator(llm_provider),
        );
        self
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
