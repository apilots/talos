//! Talos LLM DeepSeek provider.

use async_trait::async_trait;
use reqwest::{header, Client, Response};
use serde_json::Value;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::{chat as chat_protocol, messages as messages_protocol},
    ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelBlockStream, ReasoningEffort,
};
use tracing::info;

/// DeepSeek wire protocol selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepSeekProtocol {
    Chat,
    Messages,
}

/// DeepSeek API configuration.
#[derive(Debug, Clone)]
pub struct DeepSeekConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
    pub default_model: String,
    pub protocol: DeepSeekProtocol,
}

impl DeepSeekConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.deepseek.com".to_string(),
            timeout_secs: 60,
            default_model: "deepseek-chat".to_string(),
            protocol: DeepSeekProtocol::Chat,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }

    pub fn with_protocol(mut self, protocol: DeepSeekProtocol) -> Self {
        self.protocol = protocol;
        self
    }
}

/// DeepSeek LLM provider.
pub struct DeepSeekProvider {
    config: DeepSeekConfig,
    client: Client,
}

impl DeepSeekProvider {
    pub fn new(config: DeepSeekConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build DeepSeek HTTP client");
        Self { config, client }
    }

    pub(crate) fn build_chat_request(
        mut request: ChatRequest,
        stream: bool,
    ) -> TalosResult<chat_protocol::ChatCompletionRequest> {
        let effort = Self::reasoning_effort(request.reasoning_effort);
        if effort.is_some() {
            request.temperature = None;
            request.top_p = None;
        }
        let thinking = effort
            .as_ref()
            .map(|_| chat_protocol::ChatCompletionThinkingConfig {
                r#type: "enabled".to_string(),
            });
        chat_protocol::request(
            request,
            stream,
            chat_protocol::ChatCompletionRequestOptions {
                reasoning_effort: effort,
                thinking,
                reasoning_continuation:
                    chat_protocol::ChatReasoningContinuationFormat::ReasoningContent,
                ..Default::default()
            },
        )
    }

    pub(crate) fn build_messages_request(
        request: ChatRequest,
        stream: Option<bool>,
    ) -> TalosResult<messages_protocol::MessagesRequest> {
        let effort = Self::reasoning_effort(request.reasoning_effort);
        let thinking = effort
            .as_ref()
            .map(|_| messages_protocol::MessagesThinkingConfig {
                r#type: "enabled".to_string(),
                budget_tokens: None,
            });
        let output_config = effort.map(|effort| messages_protocol::MessagesOutputConfig { effort });
        messages_protocol::request(
            request,
            stream,
            messages_protocol::MessagesRequestOptions {
                thinking,
                output_config,
            },
        )
    }

    fn reasoning_effort(reasoning_effort: Option<ReasoningEffort>) -> Option<String> {
        reasoning_effort.map(|effort| {
            match effort {
                ReasoningEffort::Light | ReasoningEffort::Medium | ReasoningEffort::High => "high",
                ReasoningEffort::XHigh => "max",
            }
            .to_string()
        })
    }

    fn endpoint_url(&self, endpoint: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        )
    }

    fn build_headers(&self) -> TalosResult<header::HeaderMap> {
        let mut headers = header::HeaderMap::new();
        let key = match self.config.protocol {
            DeepSeekProtocol::Chat => format!("Bearer {}", self.config.api_key),
            DeepSeekProtocol::Messages => self.config.api_key.clone(),
        };
        let key = header::HeaderValue::from_str(&key).map_err(|error| {
            TalosError::InvalidConfiguration(format!(
                "DeepSeek API key cannot be encoded as an HTTP header: {error}"
            ))
        })?;
        match self.config.protocol {
            DeepSeekProtocol::Chat => {
                headers.insert(header::AUTHORIZATION, key);
            }
            DeepSeekProtocol::Messages => {
                headers.insert("x-api-key", key);
                headers.insert(
                    "anthropic-version",
                    header::HeaderValue::from_static("2023-06-01"),
                );
            }
        }
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static(concat!("talos/", env!("CARGO_PKG_VERSION"))),
        );
        Ok(headers)
    }

    fn request_body(
        &self,
        request: ChatRequest,
        stream: bool,
    ) -> TalosResult<(&'static str, Value)> {
        let (endpoint, body) = match self.config.protocol {
            DeepSeekProtocol::Chat => (
                "chat/completions",
                serde_json::to_value(Self::build_chat_request(request, stream)?),
            ),
            DeepSeekProtocol::Messages => (
                "anthropic/v1/messages",
                serde_json::to_value(Self::build_messages_request(
                    request,
                    stream.then_some(true),
                )?),
            ),
        };
        let body = body.map_err(|error| {
            TalosError::InternalError(format!("DeepSeek request serialization failed: {error}"))
        })?;
        Ok((endpoint, body))
    }

    async fn post(&self, endpoint: &str, body: Value) -> TalosResult<Response> {
        let response = self
            .client
            .post(self.endpoint_url(endpoint))
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                TalosError::InternalError(format!("DeepSeek {endpoint} request failed: {error}"))
            })?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let message = response
            .text()
            .await
            .unwrap_or_else(|_| "response body unavailable".to_string());
        Err(TalosError::InternalError(format!(
            "DeepSeek API error for {endpoint}: {status} - {message}"
        )))
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let protocol = self.config.protocol;
        let (endpoint, body) = self.request_body(request, false)?;
        let response = self.post(endpoint, body).await?;
        match protocol {
            DeepSeekProtocol::Chat => {
                let response = response
                    .json::<chat_protocol::ChatCompletionResponse>()
                    .await
                    .map_err(|error| {
                        TalosError::InternalError(format!(
                            "DeepSeek Chat response parsing failed: {error}"
                        ))
                    })?;
                chat_protocol::chat_response(response, "DeepSeek")
            }
            DeepSeekProtocol::Messages => {
                let response = response
                    .json::<messages_protocol::MessagesResponse>()
                    .await
                    .map_err(|error| {
                        TalosError::InternalError(format!(
                            "DeepSeek Messages response parsing failed: {error}"
                        ))
                    })?;
                messages_protocol::chat_response(response)
            }
        }
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<Response> {
        let (endpoint, body) = self.request_body(request, true)?;
        self.post(endpoint, body).await
    }
}

#[async_trait]
impl LlmProvider for DeepSeekProvider {
    fn name(&self) -> &str {
        "deepseek"
    }

    fn default_model(&self) -> String {
        self.config.default_model.clone()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!(protocol = ?self.config.protocol, "Sending chat request to DeepSeek API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            DeepSeekProtocol::Chat => Ok(chat_protocol::stream(response, "DeepSeek")),
            DeepSeekProtocol::Messages => Ok(messages_protocol::stream(response, "DeepSeek")),
        }
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            DeepSeekProtocol::Chat => Ok(chat_protocol::model_blocks(
                response,
                chat_protocol::ChatCompletionModelBlocks::deepseek(),
            )),
            DeepSeekProtocol::Messages => Ok(messages_protocol::model_blocks(
                response, "DeepSeek", "deepseek",
            )),
        }
    }
}

#[cfg(test)]
#[path = "deepseek_tests.rs"]
mod tests;
