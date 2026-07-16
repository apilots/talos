//! Talos LLM Anthropic provider.

use async_trait::async_trait;
use reqwest::{header, Client};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::messages as messages_protocol, ChatRequest, ChatResponse, ChatStream, LlmProvider,
    ModelBlockStream, PromptCacheSupport, ReasoningEffort,
};
use tracing::{debug, error, info};

/// Anthropic API configuration.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
    pub default_model: Option<String>,
}

impl AnthropicConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            timeout_secs: 120,
            default_model: None,
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

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnthropicEndpointKind {
    Official,
    Compatible,
}

/// Anthropic LLM provider.
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .referer(false)
            .build()
            .expect("Failed to build HTTP client");

        Self { config, client }
    }

    fn endpoint_kind_for_base_url(base_url: &str) -> AnthropicEndpointKind {
        let base_url = base_url.trim_end_matches('/');
        if base_url.contains("api.anthropic.com") {
            AnthropicEndpointKind::Official
        } else {
            AnthropicEndpointKind::Compatible
        }
    }

    fn endpoint_kind(&self) -> AnthropicEndpointKind {
        Self::endpoint_kind_for_base_url(&self.config.base_url)
    }

    fn insert_header_value(
        headers: &mut header::HeaderMap,
        name: &'static str,
        value: impl AsRef<str>,
    ) {
        if let Ok(header_value) = header::HeaderValue::from_str(value.as_ref()) {
            headers.insert(name, header_value);
        }
    }

    fn insert_talos_headers(headers: &mut header::HeaderMap) {
        headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
        headers.insert("x-talos-client", header::HeaderValue::from_static("talos"));
        headers.insert(
            "x-talos-version",
            header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        );
        Self::insert_header_value(
            headers,
            header::USER_AGENT.as_str(),
            format!("talos/{}", env!("CARGO_PKG_VERSION")),
        );
    }

    pub(crate) fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();

        match self.endpoint_kind() {
            AnthropicEndpointKind::Official => {
                headers.insert(
                    "x-api-key",
                    header::HeaderValue::from_str(&self.config.api_key).unwrap(),
                );
                headers.insert(
                    "anthropic-version",
                    header::HeaderValue::from_static("2023-06-01"),
                );
            }
            AnthropicEndpointKind::Compatible => {
                headers.insert(
                    header::AUTHORIZATION,
                    header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
                        .unwrap(),
                );
            }
        }

        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        Self::insert_talos_headers(&mut headers);
        headers
    }

    pub(crate) fn build_request(
        request: ChatRequest,
        stream: Option<bool>,
    ) -> TalosResult<messages_protocol::MessagesRequest> {
        let thinking =
            request
                .reasoning_effort
                .map(|effort| messages_protocol::MessagesThinkingConfig {
                    r#type: "enabled".to_string(),
                    budget_tokens: Some(Self::reasoning_budget_tokens(effort)),
                });
        messages_protocol::request(
            request,
            stream,
            messages_protocol::MessagesRequestOptions {
                thinking,
                ..Default::default()
            },
        )
    }

    fn reasoning_budget_tokens(effort: ReasoningEffort) -> u32 {
        match effort {
            ReasoningEffort::Light => 1024,
            ReasoningEffort::Medium => 2048,
            ReasoningEffort::High => 4096,
            ReasoningEffort::XHigh => 8192,
        }
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let anthropic_req = Self::build_request(request, None)?;
        let url = format!("{}/messages", self.config.base_url);
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", anthropic_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&anthropic_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("Anthropic API error: {} - {}", status, error_text);
            return Err(TalosError::InternalError(format!(
                "Anthropic API error: {} - {}",
                status, error_text
            )));
        }

        let anthropic_resp: messages_protocol::MessagesResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        messages_protocol::chat_response(anthropic_resp)
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let anthropic_req = Self::build_request(request, Some(true))?;

        let url = format!("{}/messages", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&anthropic_req)
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
                "Anthropic API error: {} - {}",
                status, error_text
            )));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn default_model(&self) -> String {
        self.config
            .default_model
            .as_deref()
            .unwrap_or("claude-sonnet-4-6")
            .to_string()
    }

    fn prompt_cache_support(&self) -> PromptCacheSupport {
        PromptCacheSupport::RequestLevel
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to Anthropic API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to Anthropic API");
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        Ok(messages_protocol::stream(response, "Anthropic"))
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        Ok(messages_protocol::model_blocks(
            response,
            "Anthropic",
            "anthropic",
        ))
    }
}
