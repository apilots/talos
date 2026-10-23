use super::*;
use crate::{
    Attachment, AttachmentKind, ReasoningBlock, ReasoningContinuation, ReasoningEncryptedBlock,
    ReasoningSummaryBlock, ReasoningTextBlock,
};
use bytes::Bytes;

#[tokio::test]
async fn chat_sse_error_is_not_lost_until_end_of_stream() {
    let body = "data: {\"error\":{\"code\":\"rate_limit_exceeded\",\"message\":\"SECRET\"}}\n\n";
    let response = || reqwest::Response::from(http::Response::new(body));
    let error = stream(response(), "fixture")
        .next()
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::UsageLimitExceeded);
    assert!(!error.to_string().contains("SECRET"));
    let error = model_blocks(response(), ChatCompletionModelBlocks::openai())
        .next()
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::UsageLimitExceeded);
}

#[test]
fn request_encodes_ordered_text_image_and_file_blocks() {
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
            .model("gpt-test")
            .messages(vec![message])
            .build(),
        false,
        ChatCompletionRequestOptions::default(),
    )
    .expect("request should encode");

    assert_eq!(
        serde_json::to_value(encoded).expect("request should serialize")["messages"][0]["content"],
        serde_json::json!([
            {"type": "text", "text": "inspect"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AQID"}},
            {"type": "file", "file": {"filename": "note.pdf", "file_data": "data:application/pdf;base64,BAUG"}}
        ])
    );
}

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

#[test]
fn text_replay_refuses_nonempty_signatures_but_explicit_omit_remains_outbound_only() {
    for signature in [None, Some(""), Some("signature-evidence")] {
        let message = ChatMessage::assistant("answer").with_reasoning_continuation(
            ReasoningContinuation::Blocks(vec![ReasoningBlock::Text(ReasoningTextBlock {
                provider_item_id: Some("rs-1".into()),
                provider_index: Some(0),
                text: "thought".into(),
                signature: signature.map(str::to_owned),
                format: None,
            })]),
        );
        for format in [
            ChatReasoningContinuationFormat::Reasoning,
            ChatReasoningContinuationFormat::ReasoningContent,
            ChatReasoningContinuationFormat::Omit,
            ChatReasoningContinuationFormat::ReasoningDetails,
        ] {
            let request = ChatRequest::builder()
                .model("opaque-model")
                .messages(vec![message.clone()])
                .build();
            let encoded = request.clone();
            let result = super::request(
                encoded,
                false,
                ChatCompletionRequestOptions {
                    reasoning_continuation: format,
                    ..Default::default()
                },
            );
            if signature.is_some_and(|value| !value.is_empty())
                && matches!(
                    format,
                    ChatReasoningContinuationFormat::Reasoning
                        | ChatReasoningContinuationFormat::ReasoningContent
                )
            {
                let error = result.unwrap_err();
                assert_eq!(error.error_code(), ErrorCode::InvalidRequest);
                assert!(!error.to_string().contains("signature-evidence"));
            } else {
                let body = serde_json::to_value(result.unwrap()).unwrap();
                assert_eq!(body["messages"][0]["content"], "answer");
                if format == ChatReasoningContinuationFormat::Omit {
                    assert!(body["messages"][0].get("reasoning").is_none());
                    assert!(body["messages"][0].get("reasoning_details").is_none());
                }
                if format == ChatReasoningContinuationFormat::ReasoningDetails {
                    assert_eq!(
                        body["messages"][0]["reasoning_details"][0]["signature"],
                        signature
                            .map(serde_json::Value::from)
                            .unwrap_or(serde_json::Value::Null)
                    );
                }
            }
            assert_eq!(request.messages, vec![message.clone()]);
        }
    }
}

#[tokio::test]
async fn native_stream_modes_preserve_their_existing_reasoning_representations() {
    let sse = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({"choices":[{"delta":{"reasoning":"plain","reasoning_details":[{"type":"reasoning.encrypted","data":"opaque","id":"rs-1","index":0}]},"finish_reason":"stop"}]})
    );
    for (options, structured) in [
        (ChatCompletionModelBlocks::openai(), false),
        (ChatCompletionModelBlocks::openrouter(), true),
    ] {
        let response = reqwest::Response::from(http::Response::new(sse.clone()));
        let mut stream = model_blocks(response, options);
        let mut continuation = None;
        while let Some(event) = stream.next().await {
            if let ModelBlockStreamEvent::ReasoningCompleted { output } = event.unwrap() {
                continuation = output.continuation;
            }
        }
        if structured {
            assert!(
                matches!(continuation,Some(ReasoningContinuation::Blocks(blocks)) if matches!(&blocks[0],ReasoningBlock::Encrypted(block) if block.data=="opaque"))
            );
        } else {
            assert_eq!(
                continuation,
                Some(ReasoningContinuation::Text("plain".into()))
            );
        }
    }
}
