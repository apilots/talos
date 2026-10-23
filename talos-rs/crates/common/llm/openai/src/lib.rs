//! Talos LLM OpenAI provider.

use async_trait::async_trait;
use reqwest::{header, Client};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::{chat as chat_protocol, responses as responses_protocol},
    validate_inline_attachments, Attachment, AttachmentKind, AttachmentUsage, ChatRequest,
    ChatResponse, ChatStream, LlmProvider, ModelBlockStream, ReasoningEffort,
};
use tracing::{debug, info};

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
    pub default_model: String,
    pub protocol: OpenAIProtocol,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            timeout_secs: 60,
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

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
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
        Self::insert_talos_headers(&mut headers);

        headers
    }

    fn request_options(request: &ChatRequest) -> chat_protocol::ChatCompletionRequestOptions {
        chat_protocol::ChatCompletionRequestOptions {
            reasoning_effort: if talos_llm_provider::has_boolean_thinking_control(
                &request.request_parameters,
            ) {
                None
            } else {
                Self::effective_reasoning_effort(request.reasoning_effort)
            },
            reasoning_continuation: chat_protocol::ChatReasoningContinuationFormat::Reasoning,
            ..Default::default()
        }
    }

    fn build_request(
        request: ChatRequest,
        stream: bool,
    ) -> TalosResult<chat_protocol::ChatCompletionRequest> {
        let options = Self::request_options(&request);
        let mut encoded = chat_protocol::request(request, stream, options)?;
        chat_protocol::merge_initial_system_messages(&mut encoded);
        Ok(encoded)
    }

    fn responses_request(
        request: ChatRequest,
        stream: Option<bool>,
    ) -> TalosResult<responses_protocol::ResponsesRequest> {
        let reasoning = Self::effective_reasoning_effort(request.reasoning_effort)
            .map(|effort| responses_protocol::ResponsesReasoning { effort });
        let include = reasoning
            .as_ref()
            .map(|_| vec!["reasoning.encrypted_content".to_string()]);
        responses_protocol::request(
            request,
            stream,
            responses_protocol::ResponsesRequestOptions { reasoning, include },
        )
    }

    fn endpoint_url(&self, endpoint: &str) -> String {
        let base_url = self.config.base_url.trim_end_matches('/');
        let endpoint = endpoint.trim_start_matches('/');
        format!("{base_url}/{endpoint}")
    }

    fn convert_reasoning_effort(effort: ReasoningEffort) -> String {
        match effort {
            ReasoningEffort::Light => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::XHigh => "xhigh",
        }
        .to_string()
    }

    fn effective_reasoning_effort(reasoning_effort: Option<ReasoningEffort>) -> Option<String> {
        reasoning_effort.map(Self::convert_reasoning_effort)
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.validate_request_attachments(&request)?;
        match self.config.protocol {
            OpenAIProtocol::Chat => self.send_chat_completion(request).await,
            OpenAIProtocol::Responses => self.send_responses(request).await,
        }
    }

    async fn send_chat_completion(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let request_parameters = request.request_parameters.clone();
        let openai_req = Self::build_request(request, false)?;

        let url = self.endpoint_url("chat/completions");
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", openai_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&talos_llm_provider::serialize_with_parameters(
                &openai_req,
                &request_parameters,
            )?)
            .send()
            .await
            .map_err(|e| {
                TalosError::internal_with_source(
                    "HTTP request failed; technical details were withheld.",
                    e.without_url(),
                )
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(talos_llm_provider::http_response_error("OpenAI", &response));
        }

        let openai_resp: chat_protocol::ChatCompletionResponse =
            response.json().await.map_err(|e| {
                TalosError::internal_with_source(
                    "Failed to parse response; technical details were withheld.",
                    e.without_url(),
                )
            })?;

        chat_protocol::chat_response(openai_resp, "OpenAI")
    }

    async fn send_responses(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let request_parameters = request.request_parameters.clone();
        let openai_req = Self::responses_request(request, None)?;

        let url = self.endpoint_url("responses");
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", openai_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&talos_llm_provider::serialize_with_parameters(
                &openai_req,
                &request_parameters,
            )?)
            .send()
            .await
            .map_err(|e| {
                TalosError::internal_with_source(
                    "HTTP request failed; technical details were withheld.",
                    e.without_url(),
                )
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(talos_llm_provider::http_response_error("OpenAI", &response));
        }

        let openai_resp: responses_protocol::ResponsesResponse =
            response.json().await.map_err(|e| {
                TalosError::internal_with_source(
                    "Failed to parse response; technical details were withheld.",
                    e.without_url(),
                )
            })?;

        Ok(responses_protocol::chat_response(openai_resp))
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        self.validate_request_attachments(&request)?;
        let request_parameters = request.request_parameters.clone();
        let (url, body) = match self.config.protocol {
            OpenAIProtocol::Chat => (
                self.endpoint_url("chat/completions"),
                serde_json::to_value(Self::build_request(request, true)?).map_err(|e| {
                    TalosError::internal_with_source(
                        "Failed to serialize request; technical details were withheld.",
                        e,
                    )
                })?,
            ),
            OpenAIProtocol::Responses => (
                self.endpoint_url("responses"),
                serde_json::to_value(Self::responses_request(request, Some(true))?).map_err(
                    |e| {
                        TalosError::internal_with_source(
                            "Failed to serialize request; technical details were withheld.",
                            e,
                        )
                    },
                )?,
            ),
        };
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&talos_llm_provider::serialize_with_parameters(
                &body,
                &request_parameters,
            )?)
            .send()
            .await
            .map_err(|e| {
                TalosError::internal_with_source(
                    "HTTP request failed; technical details were withheld.",
                    e.without_url(),
                )
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(talos_llm_provider::http_response_error("OpenAI", &response));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn default_model(&self) -> String {
        self.config.default_model.clone()
    }

    /// Accepts the media types supported by OpenAI's inline file protocols.
    fn validate_attachments(
        &self,
        _model: &str,
        attachments: &[Attachment],
    ) -> TalosResult<AttachmentUsage> {
        validate_inline_attachments(attachments, |attachment| match attachment.kind {
            AttachmentKind::Image => matches!(
                attachment.media_type.as_str(),
                "image/png" | "image/jpeg" | "image/webp"
            ),
            AttachmentKind::File => matches!(
                attachment.media_type.as_str(),
                "application/msword"
                    | "application/pdf"
                    | "application/vnd.ms-excel"
                    | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                    | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    | "text/csv"
            ),
        })
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to OpenAI API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to OpenAI API");
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenAIProtocol::Chat => Ok(chat_protocol::stream(response, "OpenAI")),
            OpenAIProtocol::Responses => Ok(responses_protocol::stream(response, "OpenAI")),
        }
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenAIProtocol::Chat => Ok(chat_protocol::model_blocks(
                response,
                chat_protocol::ChatCompletionModelBlocks::openai(),
            )),
            OpenAIProtocol::Responses => Ok(responses_protocol::model_blocks(
                response,
                responses_protocol::ResponsesModelBlocks::openai(),
            )),
        }
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
