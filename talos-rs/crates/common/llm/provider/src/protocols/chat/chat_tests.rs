use super::*;
use crate::{
    ReasoningBlock, ReasoningContinuation, ReasoningEncryptedBlock, ReasoningSummaryBlock,
    ReasoningTextBlock,
};

#[test]
fn request_replays_text_reasoning_before_tool_call() {
    let request = request(
        ChatRequest::builder()
            .model("deepseek-v4-pro")
            .messages(vec![ChatMessage::assistant_with_tool_calls(
                "working",
                vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
            )
            .with_reasoning_continuation(ReasoningContinuation::Text(
                "reasoning".to_string(),
            ))])
            .reasoning_effort(None)
            .build(),
        false,
        ChatCompletionRequestOptions {
            reasoning_continuation: ChatReasoningContinuationFormat::ReasoningContent,
            ..Default::default()
        },
    )
    .expect("request should build");
    let actual = serde_json::to_value(request).expect("request should serialize");

    assert_eq!(actual["messages"][0]["reasoning_content"], "reasoning");
    assert_eq!(actual["messages"][0]["tool_calls"][0]["id"], "call-1");
}

#[test]
fn request_serializes_provider_owned_reasoning_controls() {
    let request = request(
        ChatRequest::builder()
            .model("deepseek-v4-pro")
            .message("hello")
            .build(),
        false,
        ChatCompletionRequestOptions {
            reasoning: Some(ChatCompletionReasoningConfig {
                effort: "high".to_string(),
            }),
            thinking: Some(ChatCompletionThinkingConfig {
                r#type: "enabled".to_string(),
            }),
            ..Default::default()
        },
    )
    .expect("request should build");
    let actual = serde_json::to_value(request).expect("request should serialize");

    assert_eq!(actual["reasoning"]["effort"], "high");
    assert_eq!(actual["thinking"]["type"], "enabled");
}

#[test]
fn request_rejects_structured_reasoning_for_text_only_field() {
    let result = request(
        ChatRequest::builder()
            .model("deepseek-v4-pro")
            .messages(vec![ChatMessage::assistant_with_tool_calls(
                "",
                vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
            )
            .with_reasoning_continuation(ReasoningContinuation::Blocks(vec![
                ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
                    provider_item_id: Some("encrypted-1".to_string()),
                    provider_index: Some(0),
                    data: "encrypted".to_string(),
                    format: Some("openrouter-v1".to_string()),
                }),
            ]))])
            .build(),
        false,
        ChatCompletionRequestOptions {
            reasoning_continuation: ChatReasoningContinuationFormat::ReasoningContent,
            ..Default::default()
        },
    );

    assert!(result.is_err());
}

#[test]
fn response_maps_reasoning_details_without_losing_order() {
    let response: ChatCompletionResponse = serde_json::from_value(serde_json::json!({
        "model": "openrouter/model",
        "choices": [{
            "message": {
                "content": "done",
                "reasoning_details": [
                    {
                        "type": "reasoning.summary",
                        "summary": "summary",
                        "id": "summary-1",
                        "format": "anthropic-claude-v1",
                        "index": 0
                    },
                    {
                        "type": "reasoning.text",
                        "text": "reasoning",
                        "signature": "signature",
                        "id": "text-1",
                        "format": "anthropic-claude-v1",
                        "index": 1
                    }
                ]
            },
            "finish_reason": "tool_calls"
        }]
    }))
    .expect("response should parse");

    let mapped = chat_response(response, "OpenRouter").expect("response should map");

    assert_eq!(
        mapped.reasoning.display_text,
        vec!["summary".to_string(), "reasoning".to_string()]
    );
    assert_eq!(
        mapped.reasoning.continuation,
        Some(ReasoningContinuation::Blocks(vec![
            ReasoningBlock::Summary(ReasoningSummaryBlock {
                provider_item_id: Some("summary-1".to_string()),
                provider_index: Some(0),
                text: "summary".to_string(),
                format: Some("anthropic-claude-v1".to_string()),
            }),
            ReasoningBlock::Text(ReasoningTextBlock {
                provider_item_id: Some("text-1".to_string()),
                provider_index: Some(1),
                text: "reasoning".to_string(),
                signature: Some("signature".to_string()),
                format: Some("anthropic-claude-v1".to_string()),
            }),
        ]))
    );
}

#[test]
fn stream_reasoning_details_merge_by_provider_index() {
    let mut details = Vec::new();

    accumulate_reasoning_detail(
        &mut details,
        ChatCompletionReasoningDetail::Text {
            text: "first ".to_string(),
            signature: None,
            id: Some("text-1".to_string()),
            format: None,
            index: Some(1),
        },
    )
    .expect("first text detail should accumulate");
    accumulate_reasoning_detail(
        &mut details,
        ChatCompletionReasoningDetail::Text {
            text: "second".to_string(),
            signature: Some("signature".to_string()),
            id: Some("text-1".to_string()),
            format: Some("anthropic-claude-v1".to_string()),
            index: Some(1),
        },
    )
    .expect("second text detail should merge");
    accumulate_reasoning_detail(
        &mut details,
        ChatCompletionReasoningDetail::Encrypted {
            data: "encrypted".to_string(),
            id: Some("encrypted-2".to_string()),
            format: Some("anthropic-claude-v1".to_string()),
            index: Some(2),
        },
    )
    .expect("encrypted detail should retain its position");

    let actual = details
        .into_iter()
        .map(ChatCompletionReasoningDetail::into_reasoning_block)
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            ReasoningBlock::Text(ReasoningTextBlock {
                provider_item_id: Some("text-1".to_string()),
                provider_index: Some(1),
                text: "first second".to_string(),
                signature: Some("signature".to_string()),
                format: Some("anthropic-claude-v1".to_string()),
            }),
            ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
                provider_item_id: Some("encrypted-2".to_string()),
                provider_index: Some(2),
                data: "encrypted".to_string(),
                format: Some("anthropic-claude-v1".to_string()),
            }),
        ]
    );
}
