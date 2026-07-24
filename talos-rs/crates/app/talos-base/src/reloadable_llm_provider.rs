use std::sync::{Arc, RwLock};

use talos_error::TalosResult;
use talos_llm_provider::{
    Attachment, AttachmentUsage, ChatRequest, ChatResponse, ChatStream, LlmProvider,
    ModelBlockStream, PromptCacheSupport,
};

/// Delegates requests to an LLM provider that can be replaced after configuration reloads.
pub(crate) struct ReloadableLlmProvider {
    current: RwLock<Arc<dyn LlmProvider>>,
}

impl ReloadableLlmProvider {
    pub(crate) fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            current: RwLock::new(provider),
        }
    }

    pub(crate) fn reload(&self, provider: Arc<dyn LlmProvider>) {
        *self.current.write().expect("llm provider write lock") = provider;
    }

    fn current(&self) -> Arc<dyn LlmProvider> {
        self.current.read().expect("llm provider read lock").clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for ReloadableLlmProvider {
    fn name(&self) -> &str {
        "reloadable"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.current().chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        self.current().stream(request).await
    }

    fn default_model(&self) -> String {
        self.current().default_model()
    }

    fn prompt_cache_support(&self) -> PromptCacheSupport {
        self.current().prompt_cache_support()
    }

    fn validate_attachments(
        &self,
        model: &str,
        attachments: &[Attachment],
    ) -> TalosResult<AttachmentUsage> {
        self.current().validate_attachments(model, attachments)
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.current().chat_with_tools(request).await
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        self.current().stream_model_blocks(request).await
    }
}
