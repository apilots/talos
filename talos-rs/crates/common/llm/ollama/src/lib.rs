//! Talos LLM Ollama provider.

use async_trait::async_trait;
use reqwest::{header, Client, Response};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::chat as chat_protocol, ChatRequest, ChatResponse, ChatStream, LlmProvider,
    ModelBlockStream, ReasoningEffort,
};

/// Ollama API configuration.
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub timeout_secs: u64,
    pub default_model: String,
}

impl OllamaConfig {
    pub fn new(base_url: impl Into<String>, default_model: impl Into<String>) -> Self {
        Self {
            api_key: None,
            base_url: base_url.into(),
            timeout_secs: 60,
            default_model: default_model.into(),
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }
}

/// Ollama LLM provider.
pub struct OllamaProvider {
    config: OllamaConfig,
    client: Client,
}

impl OllamaProvider {
    pub fn new(config: OllamaConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build Ollama HTTP client");
        Self { config, client }
    }

    fn endpoint_url(&self) -> String {
        let base_url = self.config.base_url.trim_end_matches('/');
        if base_url.ends_with("/v1") {
            format!("{base_url}/chat/completions")
        } else {
            format!("{base_url}/v1/chat/completions")
        }
    }

    fn build_headers(&self) -> TalosResult<header::HeaderMap> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        if let Some(api_key) = self.config.api_key.as_deref() {
            let authorization = header::HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|error| {
                    TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!("Ollama API key cannot be encoded as an HTTP header: {error}"),
                    )
                })?;
            headers.insert(header::AUTHORIZATION, authorization);
        }
        Ok(headers)
    }

    fn build_request(
        request: ChatRequest,
        stream: bool,
    ) -> TalosResult<chat_protocol::ChatCompletionRequest> {
        let reasoning_effort = request.reasoning_effort.map(|effort| {
            match effort {
                ReasoningEffort::Light => "low",
                ReasoningEffort::Medium => "medium",
                ReasoningEffort::High | ReasoningEffort::XHigh => "high",
            }
            .to_string()
        });
        chat_protocol::request(
            request,
            stream,
            chat_protocol::ChatCompletionRequestOptions {
                reasoning_effort,
                reasoning_continuation: chat_protocol::ChatReasoningContinuationFormat::Reasoning,
                ..Default::default()
            },
        )
    }

    async fn send(&self, request: ChatRequest, stream: bool) -> TalosResult<Response> {
        self.validate_request_attachments(&request)?;
        let body = Self::build_request(request, stream)?;
        let response = self
            .client
            .post(self.endpoint_url())
            .headers(self.build_headers()?)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Ollama Chat request failed; technical details were withheld.",
                    error.without_url(),
                )
            })?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        Err(talos_llm_provider::http_response_error("Ollama", &response))
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn default_model(&self) -> String {
        self.config.default_model.clone()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let response = self.send(request, false).await?;
        let response = response
            .json::<chat_protocol::ChatCompletionResponse>()
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Ollama Chat response parsing failed; technical details were withheld.",
                    error.without_url(),
                )
            })?;
        chat_protocol::chat_response(response, "Ollama")
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send(request, true).await?;
        Ok(chat_protocol::stream(response, "Ollama"))
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send(request, true).await?;
        Ok(chat_protocol::model_blocks(
            response,
            chat_protocol::ChatCompletionModelBlocks::ollama(),
        ))
    }
}

#[cfg(test)]
#[path = "ollama_tests.rs"]
mod tests;
