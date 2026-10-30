//! Bind canonical per-model request extensions to a configured connection.
use std::{collections::BTreeMap, sync::Arc};
use talos_config::{ProviderConfig, ProviderModelConfig};
use talos_error::TalosResult;
use talos_llm_provider::{
    apply_thinking_selection, Attachment, AttachmentUsage, ChatRequest, ChatResponse, ChatStream,
    LlmProvider, ModelBlockStream, PromptCacheSupport,
};

pub(crate) struct ConfiguredProvider {
    pub id: String,
    pub inner: Arc<dyn LlmProvider>,
    pub models: BTreeMap<String, ProviderModelConfig>,
}

impl ConfiguredProvider {
    pub fn new(id: String, inner: Arc<dyn LlmProvider>, config: &ProviderConfig) -> Self {
        Self {
            id,
            inner,
            models: config
                .models
                .iter()
                .map(|model| (model.id.clone(), model.clone()))
                .collect(),
        }
    }

    fn prepare(&self, mut request: ChatRequest) -> TalosResult<ChatRequest> {
        if request.configured_parameters_applied {
            return Ok(request);
        }
        let model = self.models.get(&request.model);
        if let Some(model) = model {
            model.validate_reasoning_effort(request.reasoning_effort)?;
        }
        request.request_parameters = model
            .map(|model| model.request_parameters.clone())
            .unwrap_or_default();
        apply_thinking_selection(
            &mut request.request_parameters,
            request.reasoning_effort.is_some(),
        );
        // An explicit binary-only declaration controls serialization, independently
        // of whether a thinking switch happened to be configured.
        if model.is_some_and(|model| model.reasoning_efforts.as_ref().is_some_and(Vec::is_empty)) {
            request.reasoning_effort = None;
        }
        request.configured_parameters_applied = true;
        Ok(request)
    }
}

#[async_trait::async_trait]
impl LlmProvider for ConfiguredProvider {
    fn bind_request(&self, request: ChatRequest) -> TalosResult<ChatRequest> {
        self.inner.bind_request(self.prepare(request)?)
    }

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
        self.inner.chat(self.prepare(request)?).await
    }
    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.inner.chat_with_tools(self.prepare(request)?).await
    }
    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        self.inner.stream(self.prepare(request)?).await
    }
    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        self.inner.stream_model_blocks(self.prepare(request)?).await
    }
}

#[cfg(test)]
#[path = "configured_provider_tests.rs"]
mod tests;
