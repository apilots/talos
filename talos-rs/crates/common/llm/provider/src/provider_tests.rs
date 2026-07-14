use super::*;
use talos_error::TalosResult;

#[test]
fn test_chat_message() {
    let msg = ChatMessage::user("hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content, "hello");
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
    assert_eq!(req.reasoning, EffectiveReasoning::Unsupported);
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
