use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};

use crate::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter, ModelBlockStream,
    ModelRef, PromptCacheSupport,
};

/// Factory used by the reloadable router to build a provider for a model ref.
///
/// Implementations live at the application boundary because concrete provider
/// construction depends on runtime configuration and provider crates. The LLM
/// provider crate owns only the cache and routing behavior.
pub type LlmProviderFactory = dyn Fn(&ModelRef) -> TalosResult<Arc<dyn LlmProvider>> + Send + Sync;

/// Reloadable provider router shared by application entry points.
///
/// The router caches concrete providers by provider id and wraps each resolved
/// provider with the selected model id. Callers remain responsible for model
/// selection priority; this type only executes an already-resolved ModelRef.
pub struct ReloadableLlmProviderRouter {
    provider_factory: Arc<LlmProviderFactory>,
    providers: RwLock<HashMap<String, Arc<dyn LlmProvider>>>,
}

impl ReloadableLlmProviderRouter {
    pub fn new(provider_factory: Arc<LlmProviderFactory>) -> Self {
        Self {
            provider_factory,
            providers: RwLock::new(HashMap::new()),
        }
    }

    pub fn reload(&self) {
        self.providers
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn cached_provider_for(&self, model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        let provider_name = model_ref.provider.trim().to_ascii_lowercase();
        if provider_name.is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "model_ref.provider must not be empty".to_string(),
            ));
        }
        if model_ref.id.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "model_ref.id must not be empty".to_string(),
            ));
        }

        if let Some(provider) = self
            .providers
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&provider_name)
            .cloned()
        {
            return Ok(provider);
        }

        let provider = (self.provider_factory)(model_ref)?;
        self.providers
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(provider_name, provider.clone());
        Ok(provider)
    }
}

impl LlmProviderRouter for ReloadableLlmProviderRouter {
    fn provider_for(&self, model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        let provider = self.cached_provider_for(model_ref)?;
        Ok(Arc::new(SelectedModelLlmProvider {
            inner: provider,
            model: model_ref.id.clone(),
        }))
    }
}

struct SelectedModelLlmProvider {
    inner: Arc<dyn LlmProvider>,
    model: String,
}

#[async_trait]
impl LlmProvider for SelectedModelLlmProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.inner.chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        self.inner.stream(request).await
    }

    fn default_model(&self) -> String {
        self.model.clone()
    }

    fn prompt_cache_support(&self) -> PromptCacheSupport {
        self.inner.prompt_cache_support()
    }

    fn validate_attachments(
        &self,
        model: &str,
        attachments: &[crate::Attachment],
    ) -> TalosResult<crate::AttachmentUsage> {
        self.inner.validate_attachments(model, attachments)
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.inner.chat_with_tools(request).await
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        self.inner.stream_model_blocks(request).await
    }
}
