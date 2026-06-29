//! Talos LLM OpenAI provider.

use async_trait::async_trait;
use reqwest::{header, Client};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::{chat as chat_protocol, responses as responses_protocol},
    ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelBlockStream, ReasoningEffort,
    ReasoningSupport,
};
use tracing::{debug, error, info};

/// OpenAI wire protocol selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAIProtocol {
    Chat,
    Responses,
}

/// OpenAI API configuration.
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
    pub provider_name: String,
    pub display_name: String,
    pub item_prefix: String,
    pub default_model: String,
    pub protocol: OpenAIProtocol,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            timeout_secs: 60,
            provider_name: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            item_prefix: "openai".to_string(),
            default_model: "gpt-5.5".to_string(),
            protocol: OpenAIProtocol::Chat,
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

    pub fn with_identity(
        mut self,
        provider_name: impl Into<String>,
        display_name: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        self.provider_name = provider_name.into();
        self.display_name = display_name.into();
        self.item_prefix = self.provider_name.clone();
        self.default_model = default_model.into();
        self
    }

    pub fn with_protocol(mut self, protocol: OpenAIProtocol) -> Self {
        self.protocol = protocol;
        self
    }
}

/// OpenAI LLM provider.
pub struct OpenAIProvider {
    config: OpenAIConfig,
    client: Client,
}

impl OpenAIProvider {
    pub fn new(config: OpenAIConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");

        Self { config, client }
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

    fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).unwrap(),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        Self::insert_talos_headers(&mut headers);

        headers
    }

    fn request_options(request: &ChatRequest) -> chat_protocol::ChatCompletionRequestOptions {
        chat_protocol::ChatCompletionRequestOptions {
            reasoning_effort: request.reasoning_effort.map(Self::convert_reasoning_effort),
            ..Default::default()
        }
    }

    fn build_request(request: ChatRequest, stream: bool) -> chat_protocol::ChatCompletionRequest {
        let options = Self::request_options(&request);
        chat_protocol::request(request, stream, options)
    }

    fn responses_request(
        request: ChatRequest,
        stream: Option<bool>,
    ) -> responses_protocol::ResponsesRequest {
        responses_protocol::request(request, stream)
    }

    fn endpoint_url(&self, endpoint: &str) -> String {
        let base_url = self.config.base_url.trim_end_matches('/');
        let endpoint = endpoint.trim_start_matches('/');
        format!("{base_url}/{endpoint}")
    }

    fn convert_reasoning_effort(effort: ReasoningEffort) -> String {
        chat_protocol::reasoning_effort(effort)
    }

    fn supports_reasoning_model(model: &str) -> bool {
        let normalized = model.trim().to_ascii_lowercase();
        normalized.starts_with("o1")
            || normalized.starts_with("o3")
            || normalized.starts_with("o4")
            || normalized.starts_with("gpt-5")
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        match self.config.protocol {
            OpenAIProtocol::Chat => self.send_chat_completion(request).await,
            OpenAIProtocol::Responses => self.send_responses(request).await,
        }
    }

    async fn send_chat_completion(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let openai_req = Self::build_request(request, false);

        let url = self.endpoint_url("chat/completions");
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", openai_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&openai_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            );
            return Err(TalosError::InternalError(format!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            )));
        }

        let openai_resp: chat_protocol::ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        chat_protocol::chat_response(openai_resp, &self.config.display_name)
    }

    async fn send_responses(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let openai_req = Self::responses_request(request, None);

        let url = self.endpoint_url("responses");
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", openai_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&openai_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            );
            return Err(TalosError::InternalError(format!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            )));
        }

        let openai_resp: responses_protocol::ResponsesResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        Ok(responses_protocol::chat_response(openai_resp))
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let (url, body) = match self.config.protocol {
            OpenAIProtocol::Chat => (
                self.endpoint_url("chat/completions"),
                serde_json::to_value(Self::build_request(request, true)).map_err(|e| {
                    TalosError::InternalError(format!("Failed to serialize request: {}", e))
                })?,
            ),
            OpenAIProtocol::Responses => (
                self.endpoint_url("responses"),
                serde_json::to_value(Self::responses_request(request, Some(true))).map_err(
                    |e| TalosError::InternalError(format!("Failed to serialize request: {}", e)),
                )?,
            ),
        };
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
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
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            )));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn name(&self) -> &str {
        &self.config.provider_name
    }

    fn default_model(&self) -> String {
        self.config.default_model.clone()
    }

    fn reasoning_support(&self, model: &str) -> ReasoningSupport {
        if Self::supports_reasoning_model(model) {
            ReasoningSupport::Supported
        } else {
            ReasoningSupport::Unsupported
        }
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to {} API", self.config.display_name);
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!(
            "Sending tool-aware chat request to {} API",
            self.config.display_name
        );
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenAIProtocol::Chat => Ok(chat_protocol::stream(
                response,
                self.config.display_name.clone(),
            )),
            OpenAIProtocol::Responses => Ok(responses_protocol::stream(
                response,
                self.config.display_name.clone(),
            )),
        }
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenAIProtocol::Chat => Ok(chat_protocol::model_blocks(
                response,
                chat_protocol::ChatCompletionModelBlocks::openai().with_identity(
                    self.config.display_name.clone(),
                    self.config.item_prefix.clone(),
                ),
            )),
            OpenAIProtocol::Responses => Ok(responses_protocol::model_blocks(
                response,
                responses_protocol::ResponsesModelBlocks::openai().with_identity(
                    self.config.display_name.clone(),
                    self.config.item_prefix.clone(),
                ),
            )),
        }
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
