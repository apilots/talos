//! Talos LLM Bailian provider.

use crate::usage::BailianUsage;
use async_trait::async_trait;
use reqwest::{header, Client};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{
    protocols::chat as chat_protocol, validate_inline_attachments, Attachment, AttachmentKind,
    AttachmentUsage, ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelBlockStream,
    PromptCacheSupport, ProviderHttpClients, ProviderHttpTimeouts, ReasoningEffort,
};
use tracing::info;

/// Bailian API configuration.
#[derive(Debug, Clone)]
pub struct BailianConfig {
    pub api_key: String,
    pub base_url: String,
    pub http_timeouts: Option<ProviderHttpTimeouts>,
}

impl BailianConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            http_timeouts: None,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Use the shared HTTP policy when no explicit deadlines are supplied.
    pub fn with_http_timeouts(mut self, timeouts: Option<ProviderHttpTimeouts>) -> Self {
        self.http_timeouts = timeouts;
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.http_timeouts = Some(ProviderHttpTimeouts {
            response_secs: secs,
            stream_total_secs: secs,
            ..ProviderHttpTimeouts::default()
        });
        self
    }
}

/// Bailian LLM provider.
pub struct BailianProvider {
    config: BailianConfig,
    clients: ProviderHttpClients,
}

impl BailianProvider {
    pub fn new(config: BailianConfig) -> Self {
        let clients = ProviderHttpClients::new(Client::builder, config.http_timeouts.as_ref())
            .expect("Failed to build model HTTP clients");
        Self { config, clients }
    }

    pub(crate) fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        if !self.config.api_key.is_empty() {
            headers.insert(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).unwrap(),
            );
        }
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers
    }

    fn reasoning_budget_tokens(effort: ReasoningEffort) -> u32 {
        match effort {
            ReasoningEffort::Light => 1024,
            ReasoningEffort::Medium => 2048,
            ReasoningEffort::High => 4096,
            ReasoningEffort::XHigh => 8192,
        }
    }

    pub(crate) fn build_request(
        request: ChatRequest,
        stream: bool,
    ) -> TalosResult<chat_protocol::ChatCompletionRequest> {
        let model = request.model.as_str();
        let named_effort = matches!(
            model,
            "qwen4-max-preview"
                | "qwen3.8-max"
                | "qwen3.8-max-0902"
                | "qwen3.8-flash"
                | "qwen3.8-27b"
                | "qwen3.8-2.4t-a95b"
                | "qwen3.8-omni-flash"
        );
        let can_disable_thinking = (named_effort && model != "qwen3.8-2.4t-a95b")
            || matches!(model, "qwen3.7-max" | "qwen3.7-plus" | "qwen3.6-flash");
        let parameters = &request.request_parameters;
        let configured_effort = parameters.contains_key("reasoning_effort");
        let configured_budget = parameters.contains_key("thinking_budget");
        if named_effort && configured_effort && configured_budget {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Bailian reasoning_effort and thinking_budget cannot be set together for this model.",
            ));
        }

        let mut options = chat_protocol::ChatCompletionRequestOptions {
            cache_control: true,
            reasoning_continuation:
                chat_protocol::ChatReasoningContinuationFormat::ReasoningContent,
            ..Default::default()
        };
        if named_effort && !configured_budget && !configured_effort {
            options.reasoning_effort = match request.reasoning_effort {
                Some(ReasoningEffort::Light) => Some("low".to_owned()),
                Some(ReasoningEffort::Medium) => Some("medium".to_owned()),
                Some(ReasoningEffort::High | ReasoningEffort::XHigh) => Some("xhigh".to_owned()),
                None => can_disable_thinking.then(|| "none".to_owned()),
            };
        } else if !named_effort || configured_budget {
            options.enable_thinking = request
                .reasoning_effort
                .map(|_| true)
                .or_else(|| can_disable_thinking.then_some(false));
            if !configured_budget {
                options.thinking_budget =
                    request.reasoning_effort.map(Self::reasoning_budget_tokens);
            }
        }

        let use_completion_limit = !parameters.contains_key("max_tokens")
            && (parameters.contains_key("max_completion_tokens")
                || matches!(
                    model,
                    "qwen4-max-preview" | "qwen3.8-max" | "qwen3.8-max-0902" | "qwen3.8-flash"
                ));
        let mut encoded = chat_protocol::request(request, stream, options)?;
        if use_completion_limit {
            encoded.max_completion_tokens = encoded.max_tokens.take();
        }
        Ok(encoded)
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let response = self.send_request(request, /*stream*/ false).await?;
        let bailian_resp: chat_protocol::ChatCompletionResponse<BailianUsage> = response
            .json()
            .await
            .map_err(|e| talos_llm_provider::transport_error("Bailian response decoding", e))?;

        chat_protocol::chat_response_with_usage(bailian_resp, "Bailian", BailianUsage::into_usage)
    }

    async fn send_request(
        &self,
        request: ChatRequest,
        stream: bool,
    ) -> TalosResult<reqwest::Response> {
        self.validate_request_attachments(&request)?;
        let request_parameters = request.request_parameters.clone();
        let bailian_req = Self::build_request(request, stream)?;
        let base_url = self.config.base_url.trim_end_matches('/');
        let url = format!("{base_url}/chat/completions");

        let response = self
            .clients
            .for_request(stream)
            .post(url)
            .headers(self.build_headers())
            .json(&talos_llm_provider::serialize_with_parameters(
                bailian_req,
                &request_parameters,
            )?)
            .send()
            .await
            .map_err(|e| talos_llm_provider::transport_error("Bailian HTTP request", e))?;

        if !response.status().is_success() {
            return Err(talos_llm_provider::http_rejection_error("Bailian", response).await);
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for BailianProvider {
    fn name(&self) -> &str {
        "bailian"
    }

    fn default_model(&self) -> String {
        "qwen3.7-plus".to_string()
    }

    /// Uses the shared inline Base64 upper bound for image inputs only.
    fn validate_attachments(
        &self,
        _model: &str,
        attachments: &[Attachment],
    ) -> TalosResult<AttachmentUsage> {
        if attachments
            .iter()
            .any(|attachment| attachment.kind == AttachmentKind::File)
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::UnsupportedAttachment,
                "Bailian Chat Completions does not support file attachments",
            ));
        }
        validate_inline_attachments(attachments, |attachment| {
            matches!(
                attachment.media_type.as_str(),
                "image/png" | "image/jpeg" | "image/webp"
            )
        })
    }

    fn prompt_cache_support(&self) -> PromptCacheSupport {
        PromptCacheSupport::RequestLevel
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to Bailian API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to Bailian API");
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_request(request, /*stream*/ true).await?;
        Ok(chat_protocol::stream(response, "Bailian"))
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_request(request, /*stream*/ true).await?;
        Ok(chat_protocol::model_blocks_with_usage(
            response,
            chat_protocol::ChatCompletionModelBlocks {
                provider_name: "Bailian".to_owned(),
                item_prefix: "bailian".to_owned(),
                reasoning_keys: &["reasoning_content", "thinking", "reasoning"],
                segment_after_boundary: true,
                ignore_empty_text: true,
                ignore_reasoning_after_assistant: true,
                reasoning_format: chat_protocol::ChatReasoningStreamFormat::TextAliases,
            },
            BailianUsage::into_usage,
        ))
    }
}
