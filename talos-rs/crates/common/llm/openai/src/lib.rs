//! Talos LLM OpenAI provider.

use async_trait::async_trait;
use reqwest::{header, Client};
use std::time::{Duration, Instant};
use talos_error::TalosResult;
use talos_llm_provider::{
    protocols::responses as responses_protocol, validate_inline_attachments, Attachment,
    AttachmentKind, AttachmentUsage, ChatRequest, ChatResponse, ChatStream, LlmProvider,
    ModelBlockStream, ProviderHttpClients, ProviderHttpTimeouts, ReasoningEffort,
};
use tracing::{debug, info, warn};

const GATEWAY_RETRY_COUNT: u32 = 5;
const GATEWAY_RETRY_DELAY: Duration = Duration::from_secs(5);

/// OpenAI API configuration.
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub base_url: String,
    pub http_timeouts: Option<ProviderHttpTimeouts>,
    pub default_model: String,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            http_timeouts: None,
            default_model: "gpt-5.5".to_string(),
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

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = model.into();
        self
    }
}

/// OpenAI LLM provider.
pub struct OpenAIProvider {
    config: OpenAIConfig,
    clients: ProviderHttpClients,
}

impl OpenAIProvider {
    pub fn new(config: OpenAIConfig) -> Self {
        let clients = ProviderHttpClients::new(Client::builder, config.http_timeouts.as_ref())
            .expect("Failed to build model HTTP clients");
        Self { config, clients }
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

    fn responses_request(
        &self,
        request: ChatRequest,
        stream: Option<bool>,
    ) -> TalosResult<responses_protocol::ResponsesRequest> {
        let reasoning = Self::effective_reasoning_effort(&request).map(|effort| {
            let summary =
                (effort != "none").then_some(responses_protocol::ResponsesReasoningSummary::Auto);
            responses_protocol::ResponsesReasoning { effort, summary }
        });
        let mut encoded = responses_protocol::request_with_contract(
            request,
            stream,
            responses_protocol::ResponsesRequestOptions {
                reasoning,
                include: Some(vec!["reasoning.encrypted_content".to_string()]),
            },
            responses_protocol::ResponsesContract::Official,
        )?;
        // Talos manages history locally and accepts tool schemas with optional fields.
        encoded.store = Some(false);
        for tool in encoded.tools.iter_mut().flatten() {
            tool.strict = Some(false);
            tool.parameters.get_or_insert(serde_json::Value::Null);
        }
        Ok(encoded)
    }

    fn responses_url(&self) -> String {
        let base_url = self.config.base_url.trim_end_matches('/');
        format!("{base_url}/responses")
    }

    fn convert_reasoning_effort(effort: ReasoningEffort) -> String {
        match effort {
            ReasoningEffort::Light => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::XHigh => "xhigh",
            ReasoningEffort::Max => "max",
        }
        .to_string()
    }

    fn effective_reasoning_effort(request: &ChatRequest) -> Option<String> {
        request
            .reasoning_effort
            .map(Self::convert_reasoning_effort)
            .or_else(|| {
                // GPT-6 Sol supports "none"; GPT-6.1 Sol is a distinct model that requires reasoning.
                matches!(
                    request.model.as_str(),
                    "gpt-6-luna" | "gpt-6-sol" | "gpt-6.1-terra"
                )
                .then(|| "none".to_owned())
            })
    }

    /// Retry rejected gateway requests before any response can reach the caller.
    /// Accepted responses, transport failures and stream decoding retain their
    /// original error behavior; retries never replay emitted model/tool blocks.
    async fn send_request(
        &self,
        mut request: reqwest::RequestBuilder,
    ) -> TalosResult<reqwest::Response> {
        let max_attempts = GATEWAY_RETRY_COUNT + 1;
        let mut attempt = 1;
        loop {
            let next = request.try_clone();
            let started = Instant::now();
            let response = request.send().await.map_err(|error| {
                let failure = if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "connection"
                } else {
                    "transport"
                };
                warn!(
                    event = "transport_failure",
                    protocol = "responses",
                    attempt,
                    max_attempts,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    failure,
                    "OpenAI HTTP request failed without a response"
                );
                talos_llm_provider::transport_error("OpenAI HTTP request", error)
            })?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }

            let retry =
                next.filter(|_| attempt < max_attempts && matches!(status.as_u16(), 502..=504));
            warn!(
                event = "http_rejection",
                protocol = "responses",
                attempt,
                max_attempts,
                elapsed_ms = started.elapsed().as_millis() as u64,
                http_status = status.as_u16(),
                upstream_request_id = talos_llm_provider::http_request_id(&response),
                will_retry = retry.is_some(),
                "OpenAI HTTP request rejected"
            );
            let Some(next) = retry else {
                return Err(talos_llm_provider::http_rejection_error("OpenAI", response).await);
            };
            drop(response);
            tokio::time::sleep(GATEWAY_RETRY_DELAY).await;
            request = next;
            attempt += 1;
        }
    }

    async fn send_responses(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.validate_request_attachments(&request)?;
        let request_parameters = request.request_parameters.clone();
        let openai_req = self.responses_request(request, None)?;

        let url = self.responses_url();
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", openai_req.model);

        let response = self
            .send_request(
                self.clients
                    .for_request(false)
                    .post(&url)
                    .headers(headers)
                    .json(&talos_llm_provider::serialize_with_parameters(
                        &openai_req,
                        &request_parameters,
                    )?),
            )
            .await?;

        let response = response.json().await.map_err(|error| {
            talos_llm_provider::transport_error("OpenAI response decoding", error)
        })?;
        responses_protocol::official_chat_response(response)
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        self.validate_request_attachments(&request)?;
        let request_parameters = request.request_parameters.clone();
        let url = self.responses_url();
        let body = self.responses_request(request, Some(true))?;
        let headers = self.build_headers();

        let response = self
            .send_request(
                self.clients
                    .for_request(true)
                    .post(&url)
                    .headers(headers)
                    .json(&talos_llm_provider::serialize_with_parameters(
                        &body,
                        &request_parameters,
                    )?),
            )
            .await?;

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
        self.send_responses(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to OpenAI API");
        self.send_responses(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        Ok(responses_protocol::stream_with_contract(
            response,
            "OpenAI",
            responses_protocol::ResponsesContract::Official,
        ))
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        Ok(responses_protocol::model_blocks_with_contract(
            response,
            responses_protocol::ResponsesModelBlocks::openai(),
            responses_protocol::ResponsesContract::Official,
        ))
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
