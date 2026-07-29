//! Talos LLM Bailian provider.

use async_trait::async_trait;
use reqwest::{header, Client};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::chat as chat_protocol, validate_inline_attachments, Attachment, AttachmentKind,
    AttachmentUsage, ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelBlockStream,
    PromptCacheSupport, ReasoningEffort,
};
use tracing::info;

/// Bailian API configuration.
#[derive(Debug, Clone)]
pub struct BailianConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
}

impl BailianConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            timeout_secs: 60,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BailianThinkingMode {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BailianThinkingConfig {
    pub mode: BailianThinkingMode,
    pub budget_tokens: Option<u32>,
}

/// Bailian LLM provider.
pub struct BailianProvider {
    config: BailianConfig,
    client: Client,
}

impl BailianProvider {
    pub fn new(config: BailianConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");

        Self { config, client }
    }

    pub(crate) fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).unwrap(),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers
    }

    pub(crate) fn thinking_config(request: &ChatRequest) -> Option<BailianThinkingConfig> {
        request
            .reasoning_effort
            .map(|effort| BailianThinkingConfig {
                mode: BailianThinkingMode::Enabled,
                budget_tokens: Some(Self::reasoning_budget_tokens(effort)),
            })
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
        let thinking = Self::thinking_config(&request);
        let options = chat_protocol::ChatCompletionRequestOptions {
            cache_control: true,
            enable_thinking: thinking
                .as_ref()
                .map(|config| matches!(config.mode, BailianThinkingMode::Enabled)),
            thinking_budget: thinking.as_ref().and_then(|config| config.budget_tokens),
            reasoning_continuation:
                chat_protocol::ChatReasoningContinuationFormat::ReasoningContent,
            ..Default::default()
        };
        chat_protocol::request(request, stream, options)
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.validate_request_attachments(&request)?;
        let bailian_req = Self::build_request(request, false)?;
        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&bailian_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(TalosError::InternalError(format!(
                "Bailian API error: {} - {}",
                status, error_text
            )));
        }

        let bailian_resp: chat_protocol::ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        chat_protocol::chat_response(bailian_resp, "Bailian")
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        self.validate_request_attachments(&request)?;
        let bailian_req = Self::build_request(request, true)?;
        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&bailian_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(TalosError::InternalError(format!(
                "Bailian API error: {} - {}",
                status, error_text
            )));
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
            return Err(TalosError::FileError {
                kind: talos_error::TalosFileErrorKind::UnsupportedAttachment,
                message: "Bailian Chat Completions does not support file attachments".to_string(),
            });
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
        let response = self.send_stream(request).await?;
        Ok(chat_protocol::stream(response, "Bailian"))
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        Ok(chat_protocol::model_blocks(
            response,
            chat_protocol::ChatCompletionModelBlocks::bailian(),
        ))
    }
}
