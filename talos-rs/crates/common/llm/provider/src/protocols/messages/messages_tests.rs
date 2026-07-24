use super::*;
use crate::{Attachment, AttachmentKind};
use bytes::Bytes;

#[test]
fn request_encodes_ordered_text_image_and_document_blocks() {
    let message = ChatMessage::user_with_attachments(
        "inspect",
        vec![
            Attachment {
                kind: AttachmentKind::Image,
                filename: "pixel.png".to_string(),
                media_type: "image/png".to_string(),
                bytes: Bytes::from_static(&[1, 2, 3]),
            },
            Attachment {
                kind: AttachmentKind::File,
                filename: "note.pdf".to_string(),
                media_type: "application/pdf".to_string(),
                bytes: Bytes::from_static(&[4, 5, 6]),
            },
        ],
    )
    .expect("attachments should be valid");
    let encoded = request(
        ChatRequest::builder()
            .model("claude-test")
            .messages(vec![message])
            .build(),
        None,
        MessagesRequestOptions::default(),
    )
    .expect("request should encode");

    assert_eq!(
        serde_json::to_value(encoded).expect("request should serialize")["messages"][0]["content"],
        serde_json::json!([
            {"type": "text", "text": "inspect"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AQID"}},
            {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": "BAUG"}}
        ])
    );
}
use crate::{
    ReasoningBlock, ReasoningContinuation, ReasoningEffort, ReasoningEncryptedBlock,
    ReasoningTextBlock,
};

#[test]
fn request_uses_provider_owned_thinking_options() {
    let request = request(
        ChatRequest::builder()
            .model("claude-sonnet-4-6")
            .message("hello")
            .reasoning_effort(Some(ReasoningEffort::High))
            .build(),
        Some(true),
        MessagesRequestOptions {
            thinking: Some(MessagesThinkingConfig {
                r#type: "enabled".to_string(),
                budget_tokens: Some(4096),
            }),
            ..Default::default()
        },
    )
    .expect("request should build");

    let actual = serde_json::to_value(request).expect("request should serialize");

    assert_eq!(actual["thinking"]["budget_tokens"], 4096);
}

#[test]
fn request_serializes_provider_owned_output_effort() {
    let request = request(
        ChatRequest::builder()
            .model("deepseek-v4-pro")
            .message("hello")
            .build(),
        None,
        MessagesRequestOptions {
            thinking: Some(MessagesThinkingConfig {
                r#type: "enabled".to_string(),
                budget_tokens: None,
            }),
            output_config: Some(MessagesOutputConfig {
                effort: "max".to_string(),
            }),
        },
    )
    .expect("request should build");
    let actual = serde_json::to_value(request).expect("request should serialize");

    assert_eq!(actual["thinking"]["type"], "enabled");
    assert!(actual["thinking"].get("budget_tokens").is_none());
    assert_eq!(actual["output_config"]["effort"], "max");
}

#[test]
fn assistant_replays_reasoning_blocks_before_tool_calls() {
    let continuation = ReasoningContinuation::Blocks(vec![
        ReasoningBlock::Text(ReasoningTextBlock {
            provider_item_id: Some("thinking-1".to_string()),
            provider_index: None,
            text: "reasoning".to_string(),
            signature: Some("signature".to_string()),
            format: Some("anthropic-claude-v1".to_string()),
        }),
        ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
            provider_item_id: Some("thinking-2".to_string()),
            provider_index: None,
            data: "encrypted".to_string(),
            format: Some("anthropic-claude-v1".to_string()),
        }),
    ]);
    let (_, messages) = messages(vec![ChatMessage::assistant_with_tool_calls(
        "working",
        vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
    )
    .with_reasoning_continuation(continuation)])
    .expect("messages should map");
    let blocks = messages[0]
        .content
        .as_array()
        .expect("assistant content should use blocks");

    assert_eq!(
        blocks
            .iter()
            .map(|block| block["type"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["thinking", "redacted_thinking", "text", "tool_use"]
    );
}

#[test]
fn response_separates_display_reasoning_from_replay_blocks() {
    let content = vec![
        MessagesContent {
            content_type: "thinking".to_string(),
            text: None,
            thinking: Some("visible".to_string()),
            signature: Some("signature".to_string()),
            data: None,
            id: Some("thinking-1".to_string()),
            name: None,
            input: None,
        },
        MessagesContent {
            content_type: "redacted_thinking".to_string(),
            text: None,
            thinking: None,
            signature: None,
            data: Some("encrypted".to_string()),
            id: Some("thinking-2".to_string()),
            name: None,
            input: None,
        },
    ];

    let (_, reasoning, _) = response_content(&content);

    assert_eq!(reasoning.display_text, vec!["visible".to_string()]);
    assert!(matches!(
        reasoning.continuation,
        Some(ReasoningContinuation::Blocks(blocks)) if blocks.len() == 2
    ));
}

#[test]
fn usage_includes_messages_cache_read_and_creation_tokens() {
    let usage = usage_from_messages(
        &MessagesUsage {
            input_tokens: 10,
            output_tokens: 3,
            cache_creation_input_tokens: Some(20),
            cache_read_input_tokens: Some(30),
        },
        3,
    )
    .expect("usage should map");

    assert_eq!(usage.prompt_tokens, 60);
    assert_eq!(usage.completion_tokens, 3);
    assert_eq!(usage.total_tokens, 63);
    assert_eq!(usage.cache_creation_input_tokens, Some(20));
    assert_eq!(usage.cache_read_input_tokens, Some(30));
}
