use crate::CoordinatorImpl;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider};
use talos_session_store::SessionStore;
use talos_types::gateway::GatewayRequest;
use talos_types::talos::{TalosContext, TalosInputItem, TalosProvider, TalosUsage};
use talos_types::{
    SessionPatch, SessionPatchAck, SessionQuery, SessionRecord, TalosConfig, TalosSession,
    DEFAULT_SESSION_TITLE,
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
        TalosSession::restore(record, &self.base_config, workspace_id)
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
        let session = self.build_session(&record, Some(workspace_id))?;
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
            session.state.usage.input_tokens = session
                .state
                .usage
                .input_tokens
                .saturating_add(usage.input_tokens);
            session.state.usage.output_tokens = session
                .state
                .usage
                .output_tokens
                .saturating_add(usage.output_tokens);
            session.state.usage.total_tokens = session
                .state
                .usage
                .total_tokens
                .saturating_add(usage.total_tokens);
            session.config.agent.model = assistant_model.to_string();
            session.config.llm.model = assistant_model.to_string();
        }
    }

    pub(super) async fn create_session(
        &self,
        context: TalosContext,
        target_agent: &str,
        model: Option<String>,
    ) -> TalosResult<TalosSession> {
        let record = self
            .session_store
            .create_session(
                &context,
                target_agent,
                self.base_model(),
                model.as_deref(),
                self.resolve_dm_scope(&context.provider),
            )
            .await?;
        let session = self.build_session(&record, Some(self.default_workspace_id.clone()))?;
        Ok(self.cache_session(session).await)
    }

    pub(super) async fn list_sessions(
        &self,
        query: SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
        let records = self.session_store.list_session_records(&query).await?;
        records
            .iter()
            .map(|record| self.build_session(record, Some(self.default_workspace_id.clone())))
            .collect()
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
        let session = self.build_session(
            &record,
            workspace_id.or_else(|| Some(self.default_workspace_id.clone())),
        )?;
        Ok(Some(self.cache_session(session).await))
    }

    pub(super) async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let ack = self
            .session_store
            .patch_session_record(session_id, &patch)
            .await?;
        if ack.success {
            self.invalidate_cached_session(session_id).await;
        }
        Ok(ack)
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
