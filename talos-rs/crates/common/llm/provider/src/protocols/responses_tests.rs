use super::*;
use crate::{Attachment, AttachmentKind};
use bytes::Bytes;

#[test]
fn request_encodes_ordered_text_image_and_file_blocks() {
    let message = ChatMessage::user_with_attachments(
        "inspect",
        vec![
            Attachment {
                file_id: Some("local-observation-id".into()),
                kind: AttachmentKind::Image,
                filename: "pixel.png".to_string(),
                media_type: "image/png".to_string(),
                bytes: Bytes::from_static(&[1, 2, 3]),
            },
            Attachment {
                file_id: Some("local-observation-id".into()),
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
        None,
        ResponsesRequestOptions::default(),
    )
    .expect("request should encode");

    assert_eq!(
        serde_json::to_value(encoded).expect("request should serialize")["input"][0]["content"],
        serde_json::json!([
            {"type": "input_text", "text": "inspect"},
            {"type": "input_image", "image_url": "data:image/png;base64,AQID"},
            {"type": "input_file", "filename": "note.pdf", "file_data": "data:application/pdf;base64,BAUG"}
        ])
    );
}
use crate::{
    ReasoningBlock, ReasoningContinuation, ReasoningEffort, ReasoningEncryptedBlock,
    ReasoningSummaryBlock,
};

#[test]
fn responses_request_serializes_tools_reasoning_and_function_output() {
    let continuation = ReasoningContinuation::Blocks(vec![
        ReasoningBlock::Summary(ReasoningSummaryBlock {
            provider_item_id: Some("rs_1".to_string()),
            provider_index: None,
            text: "summary".to_string(),
            format: Some("openai-responses-v1".to_string()),
        }),
        ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
            provider_item_id: Some("rs_1".to_string()),
            provider_index: None,
            data: "encrypted".to_string(),
            format: Some("openai-responses-v1".to_string()),
        }),
    ]);
    let request = request(
        ChatRequest::builder()
            .model("gpt-5")
            .messages(vec![
                ChatMessage::system("System"),
                ChatMessage::user("Hello"),
                ChatMessage::assistant_with_tool_calls(
                    "Using a tool",
                    vec![FunctionCall::new(
                        "call_1",
                        "read",
                        serde_json::json!({"path": "/tmp/a"}),
                    )],
                )
                .with_reasoning_continuation(continuation),
                ChatMessage::tool("call_1", "read", "file body"),
            ])
            .max_tokens(100)
            .tools(vec![ToolDefinition::new(
                "read",
                "Read file",
                Some(serde_json::json!({"type": "object"})),
            )])
            .tool_choice(ToolChoice::Named {
                name: "read".to_string(),
            })
            .parallel_tool_calls(true)
            .reasoning_effort(Some(ReasoningEffort::High))
            .build(),
        Some(true),
        ResponsesRequestOptions {
            reasoning: Some(ResponsesReasoning {
                effort: "high".to_string(),
            }),
            include: Some(vec!["reasoning.encrypted_content".to_string()]),
        },
    )
    .expect("request should build");

    let json = serde_json::to_value(&request).expect("request should serialize");

    assert_eq!(json["model"], "gpt-5");
    assert_eq!(json["max_output_tokens"], 100);
    assert_eq!(json["stream"], true);
    assert_eq!(json["tools"][0]["type"], "function");
    assert_eq!(json["tool_choice"]["name"], "read");
    assert_eq!(json["reasoning"]["effort"], "high");
    assert_eq!(json["include"][0], "reasoning.encrypted_content");
    assert_eq!(json["input"][2]["type"], "reasoning");
    assert_eq!(json["input"][2]["id"], "rs_1");
    assert_eq!(json["input"][4]["type"], "function_call");
    assert!(json["input"]
        .as_array()
        .expect("input should be array")
        .iter()
        .any(|item| item["type"] == "function_call_output" && item["call_id"] == "call_1"));
}

#[test]
fn responses_chat_response_reads_output_items() {
    let response: ResponsesResponse = serde_json::from_value(serde_json::json!({
        "id": "resp_1",
        "model": "gpt-5",
        "status": "completed",
        "output": [
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "thinking"}]
            },
            {
                "type": "message",
                "content": [{"type": "output_text", "text": "hello"}]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read",
                "arguments": "{\"path\":\"/tmp/a\"}"
            }
        ],
        "usage": {
            "input_tokens": 1,
            "output_tokens": 2,
            "total_tokens": 3,
            "input_tokens_details": {"cached_tokens": 1}
        }
    }))
    .expect("response should parse");

    let mapped = chat_response(response);

    assert_eq!(mapped.content, "hello");
    assert_eq!(mapped.reasoning.display_text, vec!["thinking".to_string()]);
    assert!(matches!(
        mapped.reasoning.continuation,
        Some(ReasoningContinuation::Blocks(blocks)) if blocks.len() == 1
    ));
    assert_eq!(mapped.usage.total_tokens, 3);
    assert_eq!(mapped.usage.cache_read_input_tokens, Some(1));
    let calls = mapped.tool_calls.expect("tool calls should exist");
    assert_eq!(calls[0].name, "read");
    assert_eq!(calls[0].arguments["path"], "/tmp/a");
    assert_eq!(mapped.finish_reason.as_deref(), Some("stop"));
}

#[test]
fn responses_chat_response_does_not_duplicate_output_text() {
    let response: ResponsesResponse = serde_json::from_value(serde_json::json!({
        "id": "resp_1",
        "model": "gpt-5",
        "status": "completed",
        "output_text": "hello",
        "output": [
            {
                "type": "message",
                "content": [{"type": "output_text", "text": "hello"}]
            }
        ]
    }))
    .expect("response should parse");

    let mapped = chat_response(response);

    assert_eq!(mapped.content, "hello");
}

#[tokio::test]
async fn reasoning_and_text_output_indices_do_not_create_gaps_in_tool_ordinals() {
    let events = [
        serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs-0"}}),
        serde_json::json!({"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs-0","encrypted_content":"opaque"}}),
        serde_json::json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"fc-1","call_id":"call-1","name":"echo","arguments":""}}),
        serde_json::json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"n\":1}"}),
        serde_json::json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":"fc-1"}}),
        serde_json::json!({"type":"response.output_item.added","output_index":2,"item":{"type":"message","id":"msg-2"}}),
        serde_json::json!({"type":"response.output_item.done","output_index":2,"item":{"type":"message","id":"msg-2"}}),
        serde_json::json!({"type":"response.output_item.added","output_index":3,"item":{"type":"function_call","id":"fc-3","call_id":"call-3","name":"echo","arguments":""}}),
        serde_json::json!({"type":"response.function_call_arguments.delta","output_index":3,"delta":"{\"n\":3}"}),
        serde_json::json!({"type":"response.output_item.done","output_index":3,"item":{"type":"function_call","id":"fc-3"}}),
        serde_json::json!({"type":"response.completed","response":{"status":"completed"}}),
    ];
    let sse = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    for options in [
        ResponsesModelBlocks::openai(),
        ResponsesModelBlocks::openrouter(),
    ] {
        let prefix = options.item_prefix.clone();
        let response = reqwest::Response::from(http::Response::new(sse.clone()));
        let mut stream = model_blocks(response, options);
        let mut calls = Vec::new();
        let mut arguments = Vec::new();
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ModelBlockStreamEvent::BlockStarted(block)
                    if block.kind == ModelBlockKind::ToolCall =>
                {
                    let tool = block.tool_call.unwrap();
                    calls.push((tool.index, tool.call_id.unwrap(), block.model_item_id));
                }
                ModelBlockStreamEvent::BlockDelta {
                    model_item_id,
                    delta: ModelBlockDelta::ToolArguments { delta },
                } => arguments.push((model_item_id, delta)),
                _ => {}
            }
        }
        assert_eq!(
            calls,
            vec![
                (0, "call-1".into(), format!("{prefix}-response-1-tool")),
                (1, "call-3".into(), format!("{prefix}-response-3-tool"))
            ]
        );
        assert_eq!(
            arguments,
            vec![
                (format!("{prefix}-response-1-tool"), "{\"n\":1}".into()),
                (format!("{prefix}-response-3-tool"), "{\"n\":3}".into())
            ]
        );
    }
}
