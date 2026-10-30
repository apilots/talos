//! HTTP transport for explicitly configured wire protocols, without vendor routing.
use std::collections::BTreeMap;

use async_trait::async_trait;
use reqwest::{header, Client, Response};
use serde_json::Value;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    LlmProtocol, ProviderConfig, ProviderHttpTimeouts, ProviderModelConfig, ReasoningEffort,
    ReasoningReplay,
};

use crate::{
    has_boolean_thinking_control,
    protocols::{chat, messages, responses},
    serialize_with_parameters, validate_inline_attachments, Attachment, AttachmentKind,
    AttachmentUsage, ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelBlockStream,
    ProviderHttpClients,
};

/// A custom connection whose only wire contract is its selected protocol.
pub struct ProtocolProvider {
    name: String,
    protocol: LlmProtocol,
    base_url: String,
    api_key: Option<String>,
    default_model: String,
    models: BTreeMap<String, ProviderModelConfig>,
    clients: ProviderHttpClients,
}

impl ProtocolProvider {
    pub fn new(
        name: &str,
        protocol: LlmProtocol,
        config: &ProviderConfig,
        default_model: &str,
    ) -> TalosResult<Self> {
        let base_url = config.base_url.clone().ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Custom provider Base URL is required",
            )
        })?;
        let defaults = ProviderHttpTimeouts::default();
        let clients = ProviderHttpClients::new(
            Client::builder,
            Some(config.http_timeouts.as_ref().unwrap_or(&defaults)),
        )?;
        Ok(Self {
            name: name.to_owned(),
            protocol,
            base_url,
            api_key: config.api_key.clone(),
            default_model: default_model.to_owned(),
            models: config
                .models
                .iter()
                .map(|model| (model.id.clone(), model.clone()))
                .collect(),
            clients,
        })
    }

    fn headers(&self) -> TalosResult<header::HeaderMap> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static(concat!("talos/", env!("CARGO_PKG_VERSION"))),
        );
        if self.protocol == LlmProtocol::Messages {
            headers.insert(
                "anthropic-version",
                header::HeaderValue::from_static("2023-06-01"),
            );
        }
        if let Some(key) = self.api_key.as_deref().filter(|key| !key.is_empty()) {
            let (name, value) = if self.protocol == LlmProtocol::Messages {
                (header::HeaderName::from_static("x-api-key"), key.to_owned())
            } else {
                (header::AUTHORIZATION, format!("Bearer {key}"))
            };
            headers.insert(
                name,
                header::HeaderValue::from_str(&value).map_err(|_| {
                    TalosError::new(
                        ErrorCode::InvalidRequest,
                        "API key contains invalid HTTP header characters",
                    )
                })?,
            );
        }
        Ok(headers)
    }

    fn body(&self, request: ChatRequest, stream: bool) -> TalosResult<(&'static str, Value)> {
        self.validate_request_attachments(&request)?;
        let mut parameters = request.request_parameters.clone();
        // Messages disabled thinking is a separate union variant; enabled-only
        // budgets must not accompany it. The configured model remains unchanged.
        if self.protocol == LlmProtocol::Messages {
            if let Some(thinking) = parameters
                .get_mut("thinking")
                .and_then(Value::as_object_mut)
            {
                if thinking.get("type").and_then(Value::as_str) == Some("disabled") {
                    thinking.remove("budget_tokens");
                }
            }
        }
        let effort = request.reasoning_effort;
        let options = self.models.get(&request.model);
        if let Some(options) = options {
            options.validate_reasoning_effort(effort)?;
        }
        let reasoning = options.is_some_and(|model| model.reasoning);
        let supports_effort = reasoning
            && options.is_some_and(|model| {
                model
                    .reasoning_efforts
                    .as_ref()
                    .is_none_or(|efforts| !efforts.is_empty())
            });
        // Detect an explicit enable/disable operation independently of effort.
        // Opaque thinking objects retain their protocol-specific semantics.
        let explicit_thinking = has_boolean_thinking_control(&parameters)
            || matches!(
                parameters
                    .get("thinking")
                    .and_then(|thinking| thinking.get("type"))
                    .and_then(Value::as_str),
                Some("enabled" | "disabled")
            );
        // A thinking switch never determines effort support. For Off, omit the
        // effort when an explicit switch already carries the disable operation.
        let send_effort = supports_effort && (effort.is_some() || !explicit_thinking);
        let (endpoint, body) = match self.protocol {
            LlmProtocol::Chat => {
                let replay = match options
                    .map(|model| model.reasoning_replay)
                    .unwrap_or_default()
                {
                    ReasoningReplay::ProtocolDefault | ReasoningReplay::Reasoning => {
                        chat::ChatReasoningContinuationFormat::Reasoning
                    }
                    ReasoningReplay::Omit => chat::ChatReasoningContinuationFormat::Omit,
                    ReasoningReplay::ReasoningContent => {
                        chat::ChatReasoningContinuationFormat::ReasoningContent
                    }
                };
                let mut body = chat::request(
                    request,
                    stream,
                    chat::ChatCompletionRequestOptions {
                        reasoning_effort: send_effort
                            .then(|| wire_effort(effort, LlmProtocol::Chat)),
                        reasoning_continuation: replay,
                        ..Default::default()
                    },
                )?;
                chat::merge_initial_system_messages(&mut body);
                (
                    "chat/completions",
                    serialize_with_parameters(body, &parameters)?,
                )
            }
            LlmProtocol::Responses => {
                let body = responses::request(
                    request,
                    stream.then_some(true),
                    responses::ResponsesRequestOptions {
                        reasoning: send_effort.then(|| responses::ResponsesReasoning {
                            effort: wire_effort(effort, LlmProtocol::Responses),
                        }),
                        include: (send_effort && effort.is_some())
                            .then(|| vec!["reasoning.encrypted_content".to_owned()]),
                    },
                )?;
                ("responses", serialize_with_parameters(body, &parameters)?)
            }
            LlmProtocol::Messages => {
                let body = messages::request(
                    request,
                    stream.then_some(true),
                    messages::MessagesRequestOptions {
                        // Messages owns the adaptive mode. Preserve an explicit
                        // thinking object while generating effort independently.
                        thinking: (reasoning
                            && !explicit_thinking
                            && !parameters.contains_key("thinking"))
                        .then(|| messages::MessagesThinkingConfig {
                            r#type: if effort.is_some() {
                                "adaptive"
                            } else {
                                "disabled"
                            }
                            .to_owned(),
                            budget_tokens: None,
                        }),
                        output_config: if !send_effort {
                            None
                        } else {
                            effort.map(|effort| messages::MessagesOutputConfig {
                                effort: wire_effort(Some(effort), LlmProtocol::Messages),
                            })
                        },
                    },
                )?;
                ("messages", serialize_with_parameters(body, &parameters)?)
            }
        };
        Ok((endpoint, body))
    }

    async fn send(&self, request: ChatRequest, stream: bool) -> TalosResult<Response> {
        let model = request.model.clone();
        let (endpoint, body) = self.body(request, stream)?;
        let mut response = self
            .clients
            .for_request(stream)
            .post(format!(
                "{}/{}",
                self.base_url.trim_end_matches('/'),
                endpoint
            ))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                crate::error::transport_error("Model protocol request", error)
                    .context(format!("provider={} model={model}", self.name))
            })?;
        if response.status().is_success() {
            response
                .extensions_mut()
                .insert(crate::error::ModelRequestContext(format!(
                    "provider={} model={model}",
                    self.name
                )));
            Ok(response)
        } else {
            Err(
                crate::error::http_rejection_error("Model protocol", response)
                    .await
                    .context(format!("provider={} model={model}", self.name)),
            )
        }
    }
}

fn wire_effort(effort: Option<ReasoningEffort>, protocol: LlmProtocol) -> String {
    match effort {
        None => "none",
        Some(ReasoningEffort::Light) => "low",
        Some(ReasoningEffort::Medium) => "medium",
        Some(ReasoningEffort::High) => "high",
        Some(ReasoningEffort::XHigh) if protocol == LlmProtocol::Messages => "max",
        Some(ReasoningEffort::XHigh) => "xhigh",
    }
    .to_owned()
}

#[async_trait]
impl LlmProvider for ProtocolProvider {
    fn bind_request(&self, mut request: ChatRequest) -> TalosResult<ChatRequest> {
        if self.protocol == LlmProtocol::Chat
            && self
                .models
                .get(&request.model)
                .is_some_and(|model| model.reasoning_replay == ReasoningReplay::Omit)
        {
            for message in &mut request.messages {
                message.reasoning_continuation = None;
            }
        }
        Ok(request)
    }

    fn name(&self) -> &str {
        &self.name
    }
    fn default_model(&self) -> String {
        self.default_model.clone()
    }

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
            AttachmentKind::File if self.protocol == LlmProtocol::Messages => {
                attachment.media_type == "application/pdf"
            }
            AttachmentKind::File => matches!(
                attachment.media_type.as_str(),
                "application/pdf"
                    | "application/msword"
                    | "application/vnd.ms-excel"
                    | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                    | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    | "text/csv"
            ),
        })
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let response = self.send(request, false).await?;
        match self.protocol {
            LlmProtocol::Chat => chat::chat_response(
                response
                    .json::<chat::ChatCompletionResponse>()
                    .await
                    .map_err(|error| {
                        crate::error::transport_error("Chat protocol response decoding", error)
                    })?,
                &self.name,
            ),
            LlmProtocol::Messages => messages::chat_response(
                response
                    .json::<messages::MessagesResponse>()
                    .await
                    .map_err(|error| {
                        crate::error::transport_error("Messages protocol response decoding", error)
                    })?,
            ),
            LlmProtocol::Responses => Ok(responses::chat_response(
                response
                    .json::<responses::ResponsesResponse>()
                    .await
                    .map_err(|error| {
                        crate::error::transport_error("Responses protocol response decoding", error)
                    })?,
            )),
        }
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send(request, true).await?;
        Ok(match self.protocol {
            LlmProtocol::Chat => chat::stream(response, &self.name),
            LlmProtocol::Messages => messages::stream(response, &self.name),
            LlmProtocol::Responses => responses::stream(response, &self.name),
        })
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send(request, true).await?;
        Ok(match self.protocol {
            LlmProtocol::Chat => chat::model_blocks(
                response,
                chat::ChatCompletionModelBlocks {
                    provider_name: self.name.clone(),
                    item_prefix: self.name.clone(),
                    reasoning_keys: &["reasoning", "reasoning_content", "thinking"],
                    segment_after_boundary: false,
                    ignore_empty_text: true,
                    ignore_reasoning_after_assistant: false,
                    reasoning_format: chat::ChatReasoningStreamFormat::Auto,
                },
            ),
            LlmProtocol::Messages => messages::model_blocks(response, &self.name, &self.name),
            LlmProtocol::Responses => responses::model_blocks(
                response,
                responses::ResponsesModelBlocks {
                    provider_name: self.name.clone(),
                    item_prefix: self.name.clone(),
                },
            ),
        })
    }
}

#[cfg(test)]
#[path = "protocol_provider_tests.rs"]
mod tests;

#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
#[path = "protocol_timeout_tests.rs"]
mod timeout_tests;
