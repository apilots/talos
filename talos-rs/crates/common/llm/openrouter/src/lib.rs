//! Talos LLM OpenRouter provider.

use async_trait::async_trait;
use reqwest::{header, Client, Response};
use serde_json::Value;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::{
        chat as chat_protocol, messages as messages_protocol, responses as responses_protocol,
    },
    validate_inline_attachments, Attachment, AttachmentKind, AttachmentUsage, ChatRequest,
    ChatResponse, ChatStream, LlmProvider, ModelBlockStream, ReasoningEffort,
};
use tracing::info;

/// OpenRouter wire protocol selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRouterProtocol {
    Chat,
    Responses,
    Messages,
}

/// OpenRouter API configuration.
#[derive(Debug, Clone)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
    pub default_model: String,
    pub protocol: OpenRouterProtocol,
}

impl OpenRouterConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            timeout_secs: 60,
            default_model: "openrouter/auto".to_string(),
            protocol: OpenRouterProtocol::Chat,
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

    pub fn with_protocol(mut self, protocol: OpenRouterProtocol) -> Self {
        self.protocol = protocol;
        self
    }
}

/// OpenRouter LLM provider.
pub struct OpenRouterProvider {
    config: OpenRouterConfig,
    client: Client,
}

impl OpenRouterProvider {
    pub fn new(config: OpenRouterConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build OpenRouter HTTP client");
        Self { config, client }
    }

    pub(crate) fn build_chat_request(
        request: ChatRequest,
        stream: bool,
    ) -> TalosResult<chat_protocol::ChatCompletionRequest> {
        let reasoning = Self::reasoning_effort(request.reasoning_effort)
            .map(|effort| chat_protocol::ChatCompletionReasoningConfig { effort });
        chat_protocol::request(
            request,
            stream,
            chat_protocol::ChatCompletionRequestOptions {
                reasoning,
                reasoning_continuation:
                    chat_protocol::ChatReasoningContinuationFormat::ReasoningDetails,
                ..Default::default()
            },
        )
    }

    pub(crate) fn build_responses_request(
        request: ChatRequest,
        stream: Option<bool>,
    ) -> TalosResult<responses_protocol::ResponsesRequest> {
        let reasoning = Self::reasoning_effort(request.reasoning_effort)
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

    pub(crate) fn build_messages_request(
        request: ChatRequest,
        stream: Option<bool>,
    ) -> TalosResult<messages_protocol::MessagesRequest> {
        let output_config = Self::reasoning_effort(request.reasoning_effort)
            .map(|effort| messages_protocol::MessagesOutputConfig { effort });
        messages_protocol::request(
            request,
            stream,
            messages_protocol::MessagesRequestOptions {
                output_config,
                ..Default::default()
            },
        )
    }

    fn reasoning_effort(reasoning_effort: Option<ReasoningEffort>) -> Option<String> {
        reasoning_effort.map(|effort| {
            match effort {
                ReasoningEffort::Light => "low",
                ReasoningEffort::Medium => "medium",
                ReasoningEffort::High => "high",
                ReasoningEffort::XHigh => "xhigh",
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
        let authorization =
            header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).map_err(
                |error| {
                    TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!("OpenRouter API key cannot be encoded as an HTTP header: {error}"),
                    )
                },
            )?;
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, authorization);
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
        self.validate_request_attachments(&request)?;
        let (endpoint, body) = match self.config.protocol {
            OpenRouterProtocol::Chat => (
                "chat/completions",
                serde_json::to_value(Self::build_chat_request(request, stream)?),
            ),
            OpenRouterProtocol::Responses => (
                "responses",
                serde_json::to_value(Self::build_responses_request(
                    request,
                    stream.then_some(true),
                )?),
            ),
            OpenRouterProtocol::Messages => (
                "messages",
                serde_json::to_value(Self::build_messages_request(
                    request,
                    stream.then_some(true),
                )?),
            ),
        };
        let body = body.map_err(|error| {
            TalosError::internal_with_source(
                "OpenRouter request serialization failed; technical details were withheld.",
                error,
            )
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
                let diagnostic = format!("OpenRouter {endpoint} request failed");
                TalosError::internal_with_source(
                    "OpenRouter request failed; technical details were withheld.",
                    error.without_url(),
                )
                .context(diagnostic)
            })?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        Err(talos_llm_provider::http_response_error(
            "OpenRouter",
            &response,
        ))
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let protocol = self.config.protocol;
        let (endpoint, body) = self.request_body(request, false)?;
        let response = self.post(endpoint, body).await?;
        match protocol {
            OpenRouterProtocol::Chat => {
                let response = response
                    .json::<chat_protocol::ChatCompletionResponse>()
                    .await
                    .map_err(|error| {
                        TalosError::internal_with_source("OpenRouter Chat response parsing failed; technical details were withheld.", error.without_url())
                    })?;
                chat_protocol::chat_response(response, "OpenRouter")
            }
            OpenRouterProtocol::Responses => {
                let response = response
                    .json::<responses_protocol::ResponsesResponse>()
                    .await
                    .map_err(|error| {
                        TalosError::internal_with_source("OpenRouter Responses response parsing failed; technical details were withheld.", error.without_url())
                    })?;
                Ok(responses_protocol::chat_response(response))
            }
            OpenRouterProtocol::Messages => {
                let response = response
                    .json::<messages_protocol::MessagesResponse>()
                    .await
                    .map_err(|error| {
                        TalosError::internal_with_source("OpenRouter Messages response parsing failed; technical details were withheld.", error.without_url())
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
impl LlmProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn default_model(&self) -> String {
        self.config.default_model.clone()
    }

    /// Applies the selected OpenRouter protocol's inline media policy.
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
            AttachmentKind::File if self.config.protocol == OpenRouterProtocol::Messages => {
                attachment.media_type == "application/pdf"
            }
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
        info!(protocol = ?self.config.protocol, "Sending chat request to OpenRouter API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenRouterProtocol::Chat => Ok(chat_protocol::stream(response, "OpenRouter")),
            OpenRouterProtocol::Responses => Ok(responses_protocol::stream(response, "OpenRouter")),
            OpenRouterProtocol::Messages => Ok(messages_protocol::stream(response, "OpenRouter")),
        }
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenRouterProtocol::Chat => Ok(chat_protocol::model_blocks(
                response,
                chat_protocol::ChatCompletionModelBlocks::openrouter(),
            )),
            OpenRouterProtocol::Responses => Ok(responses_protocol::model_blocks(
                response,
                responses_protocol::ResponsesModelBlocks::openrouter(),
            )),
            OpenRouterProtocol::Messages => Ok(messages_protocol::model_blocks(
                response,
                "OpenRouter",
                "openrouter",
            )),
        }
    }
}

#[cfg(test)]
#[path = "openrouter_tests.rs"]
mod tests;
