use super::*;
use crate::{
    EffectiveReasoning, ReasoningBlock, ReasoningContinuation, ReasoningEffort,
    ReasoningEncryptedBlock, ReasoningSummaryBlock,
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
            .reasoning(EffectiveReasoning::Effort(ReasoningEffort::High))
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
