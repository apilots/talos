use super::*;
use bytes::Bytes;
use talos_error::{TalosError, TalosFileErrorKind, TalosResult};

struct DefaultAttachmentProvider;

#[async_trait::async_trait]
impl LlmProvider for DefaultAttachmentProvider {
    fn name(&self) -> &str {
        "default"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        unreachable!("chat is not used")
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        unreachable!("stream is not used")
    }

    fn default_model(&self) -> String {
        "default".to_string()
    }
}

fn png_attachment() -> Attachment {
    Attachment {
        kind: AttachmentKind::Image,
        filename: "pixel.png".to_string(),
        media_type: "image/png".to_string(),
        bytes: Bytes::from_static(&[1, 2, 3]),
    }
}

#[test]
fn test_chat_message() {
    let msg = ChatMessage::user("hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content, "hello");
    assert_eq!(msg.attachments, None);
}

#[test]
fn user_message_accepts_only_non_empty_attachments() {
    let message = ChatMessage::user_with_attachments("", vec![png_attachment()])
        .expect("user attachment should be accepted");
    assert_eq!(message.attachments, Some(vec![png_attachment()]));

    assert!(ChatMessage::user_with_attachments("hello", Vec::new()).is_err());

    let mut ambiguous = ChatMessage::user("hello");
    ambiguous.attachments = Some(Vec::new());
    assert!(ambiguous.validate_attachments().is_err());
}

#[test]
fn message_validation_rejects_attachments_on_non_user_roles() {
    let mut message = ChatMessage::assistant("answer");
    message.attachments = Some(vec![png_attachment()]);

    assert!(message.validate_attachments().is_err());
}

#[test]
fn generic_message_serde_never_persists_attachment_bytes() {
    let message = ChatMessage::user_with_attachments("look", vec![png_attachment()])
        .expect("user attachment should be accepted");
    let value = serde_json::to_value(&message).expect("message should serialize");
    assert_eq!(value.get("attachments"), None);

    let decoded: ChatMessage =
        serde_json::from_value(value).expect("message should deserialize without attachments");
    assert_eq!(decoded.attachments, None);
}

#[test]
fn provider_default_rejects_non_empty_attachments() {
    let error = DefaultAttachmentProvider
        .validate_attachments("default", &[png_attachment()])
        .expect_err("default provider must reject attachments");

    assert!(matches!(
        error,
        TalosError::FileError {
            kind: TalosFileErrorKind::UnsupportedAttachment,
            ..
        }
    ));
}

#[test]
fn inline_validation_reserves_the_per_item_context_allowance() {
    let usage = validate_inline_attachments(&[png_attachment()], |_| true)
        .expect("standard PNG should be accepted");

    assert_eq!(
        usage,
        AttachmentUsage {
            item_tokens: vec![10_000]
        }
    );
    assert_eq!(validate_attachment_usage(1, &usage).unwrap(), 10_000);
    assert!(validate_attachment_usage(
        1,
        &AttachmentUsage {
            item_tokens: vec![10_001]
        }
    )
    .is_err());
    assert!(matches!(
        validate_attachment_usage(
            1,
            &AttachmentUsage {
                item_tokens: Vec::new(),
            },
        ),
        Err(TalosError::InternalError(_))
    ));
}

#[test]
fn inline_validation_applies_provider_media_and_common_byte_limits() {
    let mut wrong_kind = png_attachment();
    wrong_kind.kind = AttachmentKind::File;
    assert!(matches!(
        validate_inline_attachments(&[wrong_kind], |attachment| {
            attachment.kind == AttachmentKind::Image
        }),
        Err(TalosError::FileError {
            kind: TalosFileErrorKind::UnsupportedMediaType,
            ..
        })
    ));

    assert!(matches!(
        validate_inline_attachments(&vec![png_attachment(); 6], |_| true),
        Err(TalosError::FileError {
            kind: TalosFileErrorKind::AttachmentLimitExceeded,
            ..
        })
    ));

    let shared = Bytes::from(vec![0_u8; 11 * 1024 * 1024]);
    let oversized_total = [
        Attachment {
            bytes: shared.clone(),
            ..png_attachment()
        },
        Attachment {
            bytes: shared,
            ..png_attachment()
        },
    ];
    assert!(matches!(
        validate_inline_attachments(&oversized_total, |_| true),
        Err(TalosError::FileError {
            kind: TalosFileErrorKind::AttachmentLimitExceeded,
            ..
        })
    ));
}

#[test]
fn test_chat_request_builder() {
    let req = ChatRequest::builder()
        .message("hello")
        .model("claude-3-5-sonnet")
        .temperature(0.7)
        .build();

    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.model, "claude-3-5-sonnet");
    assert_eq!(req.temperature, Some(0.7));
    assert_eq!(req.reasoning_effort, None);
}

#[test]
fn test_provider_config() {
    let config = ProviderRuntimeConfig::anthropic("test-key");
    assert_eq!(config.name, "anthropic");
    assert!(config.base_url.contains("anthropic.com"));
}

#[test]
fn test_function_call_roundtrip() {
    let call = FunctionCall::new("call_1", "read", serde_json::json!({"path": "/tmp/a"}));
    let encoded = serde_json::to_string(&call).unwrap();
    let decoded: FunctionCall = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, call);
}

#[test]
fn test_function_response_error() {
    let response = FunctionResponse::error("call_1", "read", serde_json::json!("failed"));
    assert!(response.is_error);
}

#[test]
fn test_tool_definition_validation() {
    let def = ToolDefinition::new(
        "read",
        "Read file",
        Some(serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {"type": "string"}
            }
        })),
    );

    assert!(def
        .validate_arguments(&serde_json::json!({"path": "/tmp/a"}))
        .is_ok());
    assert!(def.validate_arguments(&serde_json::json!({})).is_err());
}

#[test]
fn test_chat_response_tool_calls_helper() {
    let response = ChatResponse {
        content: String::new(),
        reasoning: ReasoningOutput::default(),
        model: "mock".to_string(),
        usage: Usage::default(),
        finish_reason: Some("tool_calls".to_string()),
        tool_calls: Some(vec![FunctionCall::new("1", "read", serde_json::json!({}))]),
    };

    assert!(response.has_tool_calls());
    assert_eq!(response.get_tool_calls().len(), 1);
}

#[test]
fn assistant_tool_call_message_preserves_reasoning_continuation_order() {
    let continuation = ReasoningContinuation::Blocks(vec![
        ReasoningBlock::Summary(ReasoningSummaryBlock {
            provider_item_id: Some("summary-1".to_string()),
            provider_index: Some(0),
            text: "summary".to_string(),
            format: Some("openrouter-v1".to_string()),
        }),
        ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
            provider_item_id: Some("encrypted-1".to_string()),
            provider_index: Some(1),
            data: "ciphertext".to_string(),
            format: Some("openrouter-v1".to_string()),
        }),
    ]);
    let message = ChatMessage::assistant_with_tool_calls(
        "working",
        vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
    )
    .with_reasoning_continuation(continuation.clone());

    assert_eq!(message.reasoning_continuation, Some(continuation));
}

#[test]
fn reasoning_output_separates_display_text_from_provider_continuation() {
    let output = ReasoningOutput {
        display_text: vec!["visible reasoning".to_string()],
        continuation: Some(ReasoningContinuation::Text(
            "provider replay text".to_string(),
        )),
    };

    assert_eq!(output.display_text, vec!["visible reasoning".to_string()]);
    assert_eq!(
        output.continuation,
        Some(ReasoningContinuation::Text(
            "provider replay text".to_string()
        ))
    );
}

#[test]
fn model_block_stream_can_emit_completed_reasoning_output() {
    let output = ReasoningOutput {
        display_text: vec!["visible".to_string()],
        continuation: Some(ReasoningContinuation::Text("replay".to_string())),
    };
    let event = ModelBlockStreamEvent::ReasoningCompleted {
        output: output.clone(),
    };

    assert!(matches!(
        event,
        ModelBlockStreamEvent::ReasoningCompleted { output: actual } if actual == output
    ));
}

#[tokio::test]
async fn model_block_started_can_carry_explicit_assistant_phase_hint() {
    let started = ModelBlockStarted::assistant_text(
        Some("provider_msg_1".to_string()),
        Some(AssistantTextPhaseHint::FinalAnswer),
    );

    assert_eq!(started.kind, ModelBlockKind::AssistantText);
    assert_eq!(
        started.assistant_text_phase_hint,
        Some(AssistantTextPhaseHint::FinalAnswer)
    );
    assert_eq!(
        started.assistant_text_phase_hint_source,
        Some(AssistantTextPhaseHintSource::ExplicitProviderPhase)
    );
}

#[test]
fn sse_line_decoder_buffers_split_lines() {
    let mut decoder = SseLineDecoder::new();
    assert!(decoder.push(br#"data: {"cho"#).is_empty());
    assert!(decoder.push(br#"ices":[]}"#).is_empty());
    let lines = decoder
        .push(b"\r\n\r\n")
        .into_iter()
        .collect::<TalosResult<Vec<_>>>()
        .expect("lines should decode");
    assert_eq!(
        lines,
        vec![r#"data: {"choices":[]}"#.to_string(), String::new()]
    );
    assert!(!decoder.has_pending_fragment());
}

#[test]
fn sse_line_decoder_preserves_split_utf8_codepoints() {
    let line = r#"data: {"choices":[{"delta":{"content":"中文"}}]}"#;
    let bytes = line.as_bytes();
    let split_at = line.find("文").expect("test fixture should contain text") + 1;

    let mut decoder = SseLineDecoder::new();
    assert!(decoder.push(&bytes[..split_at]).is_empty());
    let lines = decoder
        .push(&bytes[split_at..])
        .into_iter()
        .collect::<TalosResult<Vec<_>>>()
        .expect("line should not be decoded until UTF-8 is complete");
    assert!(lines.is_empty());

    let lines = decoder
        .push(b"\n")
        .into_iter()
        .collect::<TalosResult<Vec<_>>>()
        .expect("complete line should decode");
    assert_eq!(lines, vec![line.to_string()]);
    assert!(!decoder.has_pending_fragment());
}

#[test]
fn sse_line_decoder_rejects_invalid_utf8_line() {
    let mut decoder = SseLineDecoder::new();
    let lines = decoder.push(b"data: \xff\n");
    assert_eq!(lines.len(), 1);
    assert!(lines.into_iter().next().unwrap().is_err());
    assert!(!decoder.has_pending_fragment());
}
