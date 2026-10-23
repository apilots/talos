//! Bind canonical per-model request extensions to a configured connection.
use std::{collections::BTreeMap, sync::Arc};
use talos_config::ProviderConfig;
use talos_error::TalosResult;
use talos_llm_provider::{
    apply_thinking_selection, Attachment, AttachmentUsage, ChatRequest, ChatResponse, ChatStream,
    LlmProvider, ModelBlockStream, PromptCacheSupport,
};

pub(crate) struct ConfiguredProvider {
    pub id: String,
    pub inner: Arc<dyn LlmProvider>,
    pub parameters: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
}

impl ConfiguredProvider {
    pub fn new(id: String, inner: Arc<dyn LlmProvider>, config: &ProviderConfig) -> Self {
        Self {
            id,
            inner,
            parameters: config
                .models
                .iter()
                .map(|model| (model.id.clone(), model.request_parameters.clone()))
                .collect(),
        }
    }

    fn prepare(&self, mut request: ChatRequest) -> ChatRequest {
        request.request_parameters = self
            .parameters
            .get(&request.model)
            .cloned()
            .unwrap_or_default();
        apply_thinking_selection(
            &mut request.request_parameters,
            request.reasoning_effort.is_some(),
        );
        request
    }
}

#[async_trait::async_trait]
impl LlmProvider for ConfiguredProvider {
    fn name(&self) -> &str {
        &self.id
    }
    fn default_model(&self) -> String {
        self.inner.default_model()
    }
    fn prompt_cache_support(&self) -> PromptCacheSupport {
        self.inner.prompt_cache_support()
    }
    fn validate_attachments(
        &self,
        model: &str,
        attachments: &[Attachment],
    ) -> TalosResult<AttachmentUsage> {
        self.inner.validate_attachments(model, attachments)
    }
    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.inner.chat(self.prepare(request)).await
    }
    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.inner.chat_with_tools(self.prepare(request)).await
    }
    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        self.inner.stream(self.prepare(request)).await
    }
    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        self.inner.stream_model_blocks(self.prepare(request)).await
    }
}

#[cfg(test)]
#[path = "configured_provider_tests.rs"]
mod tests;
