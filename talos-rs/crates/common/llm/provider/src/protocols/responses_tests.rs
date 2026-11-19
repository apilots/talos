use super::*;
use crate::{Attachment, AttachmentKind, FunctionCall};
use crate::{ModelBlockDelta, ModelBlockKind, ModelBlockStreamEvent, ModelOutputPhase};
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
                summary: None,
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
            "input_tokens": 10,
            "output_tokens": 2,
            "output_tokens_details": {"reasoning_tokens": 1},
            "total_tokens": 12,
            "input_tokens_details": {"cached_tokens": 1, "cache_write_tokens": 6}
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
    assert_eq!(mapped.usage.total_tokens, 12);
    assert_eq!(mapped.usage.completion_tokens, 2);
    assert_eq!(mapped.usage.reasoning_tokens, Some(1));
    assert_eq!(mapped.usage.cache_creation_input_tokens, Some(6));
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

#[test]
fn encrypted_reasoning_round_trips_with_an_empty_summary() {
    let item = serde_json::json!({
        "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "opaque"
    });
    let response: ResponsesResponse = serde_json::from_value(serde_json::json!({
        "status": "completed", "output": [item]
    }))
    .unwrap();
    let mapped = chat_response(response);
    assert!(mapped.reasoning.display_text.is_empty());
    let messages = vec![ChatMessage::assistant("")
        .with_reasoning_continuation(mapped.reasoning.continuation.unwrap())];
    assert_eq!(
        input_with_contract(messages.clone(), ResponsesContract::Official).unwrap(),
        vec![item]
    );
    assert_eq!(
        input(messages).unwrap(),
        vec![serde_json::json!({"type":"reasoning","id":"rs_1","encrypted_content":"opaque"})]
    );
}

#[test]
fn responses_json_reports_terminal_outcomes_and_preserves_refusal_text() {
    let failed = serde_json::from_value(serde_json::json!({
        "status": "failed", "error": {"code": "rate_limit_exceeded", "message": "private"}
    }))
    .unwrap();
    let error = official_chat_response(failed).unwrap_err().to_api_error();
    assert_eq!(error.code, talos_error::ErrorCode::UsageLimitExceeded);
    assert!(!error.message.contains("private"));

    let refused = serde_json::from_value(serde_json::json!({
        "status": "completed",
        "output": [{"type": "message", "content": [{"type": "refusal", "refusal": "Cannot help."}]}]
    }))
    .unwrap();
    assert_eq!(
        official_chat_response(refused).unwrap().content,
        "Cannot help."
    );

    let limited = serde_json::from_value(serde_json::json!({
        "status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"},
        "output_text": "Partial text"
    }))
    .unwrap();
    let result = official_chat_response(limited).unwrap();
    assert_eq!(result.content, "Partial text");
    assert_eq!(result.finish_reason.as_deref(), Some("length"));
}

#[tokio::test]
async fn responses_streams_deliver_refusals_and_output_limit() {
    let sse = concat!(
        "data: {\"type\":\"response.refusal.delta\",\"output_index\":0,\"delta\":\"Cannot help.\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\n\n",
    );
    let mut text_stream = stream_with_contract(
        reqwest::Response::from(http::Response::new(sse)),
        "OpenAI",
        ResponsesContract::Official,
    );
    assert_eq!(
        text_stream.next().await.unwrap().unwrap().delta,
        "Cannot help."
    );
    assert_eq!(
        text_stream
            .next()
            .await
            .unwrap()
            .unwrap()
            .finish_reason
            .as_deref(),
        Some("length")
    );
    assert!(text_stream.next().await.is_none());

    let mut blocks = model_blocks_with_contract(
        reqwest::Response::from(http::Response::new(sse)),
        ResponsesModelBlocks::openai(),
        ResponsesContract::Official,
    );
    assert!(matches!(
        blocks.next().await.unwrap().unwrap(),
        ModelBlockStreamEvent::BlockStarted(_)
    ));
    assert!(matches!(
        blocks.next().await.unwrap().unwrap(),
        ModelBlockStreamEvent::BlockDelta { delta: ModelBlockDelta::AssistantText { delta }, .. }
            if delta == "Cannot help."
    ));
    assert!(matches!(
        blocks.next().await.unwrap().unwrap(),
        ModelBlockStreamEvent::BlockCompleted(_)
    ));
    assert!(matches!(
        blocks.next().await.unwrap().unwrap(),
        ModelBlockStreamEvent::Done { finish_reason, usage }
            if finish_reason.as_deref() == Some("length") && usage.as_ref().is_some_and(|usage| usage.total_tokens == 5)
    ));
    assert!(blocks.next().await.is_none());
}

#[tokio::test]
async fn responses_streams_preserve_typed_errors_and_reject_early_eof() {
    use talos_error::ErrorCode;

    for (sse, code, termination) in [
        (
            "data: {\"type\":\"error\",\"code\":\"rate_limit_exceeded\",\"message\":\"private\"}\n\n",
            ErrorCode::UsageLimitExceeded,
            None,
        ),
        (
            "data: {\"type\":\"response.in_progress\"}\n\n",
            ErrorCode::InternalError,
            Some("stream_termination=eof"),
        ),
        (
            "data: [DONE]\n\n",
            ErrorCode::InternalError,
            Some("stream_termination=premature_done_sentinel"),
        ),
    ] {
        let mut text = stream_with_contract(
            reqwest::Response::from(http::Response::new(sse)),
            "OpenAI",
            ResponsesContract::Official,
        );
        let mut blocks = model_blocks_with_contract(
            reqwest::Response::from(http::Response::new(sse)),
            ResponsesModelBlocks::openai(),
            ResponsesContract::Official,
        );
        for error in [
            text.next().await.unwrap().unwrap_err(),
            blocks.next().await.unwrap().unwrap_err(),
        ] {
            let api_error = error.to_api_error();
            assert_eq!(api_error.code, code);
            assert!(!api_error.message.contains("private"));
            if let Some(termination) = termination {
                assert!(
                    error.diagnostic_message().contains(termination),
                    "expected {termination}, got {}",
                    error.diagnostic_message(),
                );
                assert_eq!(
                    api_error.message,
                    "The model response stream ended before a terminal event."
                );
            }
        }
        assert!(text.next().await.is_none());
        assert!(blocks.next().await.is_none());
    }
}

#[tokio::test]
async fn responses_tool_completion_reconciles_arguments_before_dispatch() {
    for (final_arguments, status, expected) in [
        (r#"{"n":42}"#, "completed", Some(r#"{"n":42}"#)),
        (r#"{"m":42}"#, "completed", None),
        (r#"{"n":"#, "incomplete", None),
    ] {
        let events = [
            serde_json::json!({"type":"response.output_item.added","output_index":0,
                "item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"echo","arguments":""}}),
            serde_json::json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"n\":"}),
            serde_json::json!({"type":"response.output_item.done","output_index":0,
                "item":{"type":"function_call","arguments":final_arguments,"status":status}}),
            serde_json::json!({"type":"response.completed"}),
        ];
        let sse = events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        let mut blocks = model_blocks_with_contract(
            reqwest::Response::from(http::Response::new(sse)),
            ResponsesModelBlocks::openai(),
            ResponsesContract::Official,
        );
        let mut arguments = String::new();
        let mut completed = false;
        let mut error = None;
        while let Some(event) = blocks.next().await {
            match event {
                Ok(ModelBlockStreamEvent::BlockDelta {
                    delta: ModelBlockDelta::ToolArguments { delta },
                    ..
                }) => {
                    arguments.push_str(&delta);
                }
                Ok(ModelBlockStreamEvent::BlockCompleted(_)) => completed = true,
                Err(failure) => error = Some(failure),
                _ => {}
            }
        }
        assert_eq!(completed.then_some(arguments.as_str()), expected);
        assert_eq!(error.is_some(), expected.is_none());
    }
}

#[test]
fn compatible_responses_keep_existing_json_status_and_ignore_unknown_terminal_metadata() {
    let response = serde_json::from_value(serde_json::json!({
        "status":"incomplete",
        "output_text":"Partial text",
        "error":"provider-specific metadata",
        "incomplete_details":{"reason":"max_output_tokens"},
    }))
    .unwrap();
    let mapped = chat_response(response);
    assert_eq!(mapped.content, "Partial text");
    assert_eq!(mapped.finish_reason.as_deref(), Some("incomplete"));
}

#[tokio::test]
async fn compatible_responses_keep_streamed_tool_arguments_and_done_sentinel() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"echo\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"item_id\":\"fc_1\",\"delta\":\"{\\\"n\\\":42}\"}\n\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"arguments\":\"{ \\\"n\\\": 42 }\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"arguments\":\"{ \\\"n\\\": 42 }\"}}\n\n",
        "data: [DONE]\n\n",
    );
    let mut blocks = model_blocks(
        reqwest::Response::from(http::Response::new(sse)),
        ResponsesModelBlocks::openrouter(),
    );
    let mut arguments = String::new();
    let mut call_id = None;
    let mut finished = false;
    while let Some(event) = blocks.next().await {
        match event.unwrap() {
            ModelBlockStreamEvent::BlockStarted(block) => {
                call_id = block.tool_call.and_then(|tool| tool.call_id);
            }
            ModelBlockStreamEvent::BlockDelta {
                delta: ModelBlockDelta::ToolArguments { delta },
                ..
            } => arguments.push_str(&delta),
            ModelBlockStreamEvent::Done { finish_reason, .. } => {
                finished = finish_reason.as_deref() == Some("stop")
            }
            _ => {}
        }
    }
    assert_eq!(arguments, r#"{"n":42}"#);
    assert_eq!(call_id.as_deref(), Some("call_1"));
    assert!(finished);

    let mut text = stream(
        reqwest::Response::from(http::Response::new(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Partial\"}\n\n",
        )),
        "compatible",
    );
    assert_eq!(text.next().await.unwrap().unwrap().delta, "Partial");
    assert!(text.next().await.is_none());
}

#[tokio::test]
async fn responses_reconcile_parts_items_and_terminal_output_without_duplicates() {
    let events = [
        serde_json::json!({"type":"response.reasoning_summary_text.delta","item_id":"rs_1","summary_index":0,"delta":"Th"}),
        serde_json::json!({"type":"response.reasoning_summary_part.done","item_id":"rs_1","summary_index":0,"part":{"type":"summary_text","text":"Think"}}),
        serde_json::json!({"type":"response.output_item.done","output_index":0,"item":{"id":"rs_1","type":"reasoning","summary":[{"type":"summary_text","text":"Think"},{"type":"summary_text","text":" before acting"}],"encrypted_content":"opaque"}}),
        serde_json::json!({"type":"response.output_item.added","output_index":1,"item":{"id":"msg_1","type":"message","phase":"commentary"}}),
        serde_json::json!({"type":"response.output_text.delta","item_id":"msg_1","content_index":0,"delta":"Work"}),
        serde_json::json!({"type":"response.output_text.done","item_id":"msg_1","content_index":0,"text":"Working"}),
        serde_json::json!({"type":"response.output_item.done","output_index":1,"item":{"id":"msg_1","type":"message","phase":"commentary","content":[{"type":"output_text","text":"Working"}],"summary":[{"type":"summary_text","text":"Unrelated message metadata"}]}}),
        serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[
            {"id":"rs_1","type":"reasoning","summary":[{"type":"summary_text","text":"Think"},{"type":"summary_text","text":" before acting"}],"encrypted_content":"opaque"},
            {"id":"msg_1","type":"message","phase":"commentary","content":[{"type":"output_text","text":"Working"}]},
            {"id":"msg_2","type":"message","phase":"final_answer","content":[{"type":"output_text","text":"Done."},{"type":"refusal","refusal":"I skipped one part."}]}
        ],"usage":{"input_tokens":9,"output_tokens":7,"total_tokens":16,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":4}}}}),
    ];
    let sse = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    for contract in [ResponsesContract::Official, ResponsesContract::Compatible] {
        let response = reqwest::Response::from(http::Response::new(sse.clone()));
        let mut blocks =
            model_blocks_with_contract(response, ResponsesModelBlocks::openai(), contract);
        let mut reasoning = String::new();
        let mut text = String::new();
        let mut phases = Vec::new();
        let mut completions = 0;
        let mut finished_reasoning = None;
        let mut finish_usage = None;
        while let Some(event) = blocks.next().await {
            match event.unwrap() {
                ModelBlockStreamEvent::BlockStarted(block)
                    if block.kind == ModelBlockKind::AssistantText =>
                {
                    phases.push(block.output_phase)
                }
                ModelBlockStreamEvent::BlockDelta {
                    delta: ModelBlockDelta::AssistantText { delta },
                    ..
                } => text.push_str(&delta),
                ModelBlockStreamEvent::BlockDelta {
                    delta: ModelBlockDelta::ReasoningText { delta },
                    ..
                } => reasoning.push_str(&delta),
                ModelBlockStreamEvent::BlockCompleted(_) => completions += 1,
                ModelBlockStreamEvent::ReasoningCompleted { output } => {
                    finished_reasoning = Some(output)
                }
                ModelBlockStreamEvent::Done { usage, .. } => finish_usage = usage,
                _ => {}
            }
        }
        assert_eq!(reasoning, "Think before acting");
        let output = finished_reasoning.unwrap();
        assert_eq!(output.display_text(), reasoning);
        let Some(ReasoningContinuation::Blocks(continuation)) = output.continuation else {
            panic!("expected structured reasoning continuation");
        };
        assert_eq!(continuation.len(), 3);
        assert_eq!(text, "WorkingDone.I skipped one part.");
        assert_eq!(
            phases,
            [ModelOutputPhase::Commentary, ModelOutputPhase::FinalAnswer]
        );
        assert_eq!(completions, 3);
        let usage = finish_usage.unwrap();
        assert_eq!(usage.total_tokens, 16);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.reasoning_tokens, Some(4));
        assert_eq!(usage.cache_read_input_tokens, Some(3));

        let response = reqwest::Response::from(http::Response::new(sse.clone()));
        let mut text_stream = stream_with_contract(response, "fixture", contract);
        let mut full_text = String::new();
        while let Some(chunk) = text_stream.next().await {
            full_text.push_str(&chunk.unwrap().delta);
        }
        assert_eq!(full_text, text);
    }
}

#[tokio::test]
async fn responses_preserve_id_only_calls_and_wait_for_late_tool_metadata() {
    let events = [
        serde_json::json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{\"n\":"}),
        serde_json::json!({"type":"response.output_item.done","item":{"id":"fc_1","type":"function_call","call_id":"call_1","name":"first","arguments":"{\"n\":1}"}}),
        serde_json::json!({"type":"response.function_call_arguments.done","item_id":"fc_2","arguments":"{\"n\":2}"}),
        serde_json::json!({"type":"response.completed","response":{"output":[
            {"id":"fc_1","type":"function_call","call_id":"call_1","name":"first","arguments":"{\"n\":1}"},
            {"id":"fc_2","type":"function_call","call_id":"call_2","name":"second","arguments":{"n":2}}
        ]}}),
    ];
    let sse = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    let response = reqwest::Response::from(http::Response::new(sse));
    let mut stream = model_blocks(response, ResponsesModelBlocks::openrouter());
    let mut calls = Vec::new();
    let mut arguments = String::new();
    let mut completed_arguments = Vec::new();
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            ModelBlockStreamEvent::BlockStarted(block) => {
                let tool = block.tool_call.unwrap();
                calls.push((tool.index, tool.call_id.unwrap(), tool.tool_name.unwrap()));
            }
            ModelBlockStreamEvent::BlockDelta {
                delta: ModelBlockDelta::ToolArguments { delta },
                ..
            } => arguments.push_str(&delta),
            ModelBlockStreamEvent::BlockCompleted(_) => {
                completed_arguments.push(std::mem::take(&mut arguments))
            }
            _ => {}
        }
    }
    assert_eq!(
        calls,
        [
            (0, "call_1".into(), "first".into()),
            (1, "call_2".into(), "second".into())
        ]
    );
    assert_eq!(completed_arguments, [r#"{"n":1}"#, r#"{"n":2}"#]);
}

#[tokio::test]
async fn compatible_responses_preserve_typed_provider_errors_and_cancellation() {
    use talos_error::ErrorCode;

    for (sse, expected) in [
        (
            "event: error\ndata: {\"code\":\"rate_limit_exceeded\",\"message\":\"private\"}\n\n",
            ErrorCode::UsageLimitExceeded,
        ),
        (
            "data: {\"type\":\"response.cancelled\"}\n\n",
            ErrorCode::Cancelled,
        ),
    ] {
        let response = reqwest::Response::from(http::Response::new(sse));
        let mut blocks = model_blocks(response, ResponsesModelBlocks::openrouter());
        let error = blocks.next().await.unwrap().unwrap_err().to_api_error();
        assert_eq!(error.code, expected);
        assert!(!error.message.contains("private"));
        assert!(blocks.next().await.is_none());
    }
}

#[tokio::test]
async fn responses_keep_late_reasoning_in_continuation_after_explicit_final_answer() {
    let events = [
        serde_json::json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":"msg_1","phase":"final_answer"}}),
        serde_json::json!({"type":"response.output_text.delta","output_index":1,"delta":"Answer"}),
        serde_json::json!({"type":"response.completed","response":{"output":[
            {"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"Checked"}],"encrypted_content":"opaque"},
            {"type":"message","id":"msg_1","phase":"final_answer","content":[{"type":"output_text","text":"Answer"}]}
        ]}}),
    ];
    let sse = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    let response = reqwest::Response::from(http::Response::new(sse));
    let mut stream = model_blocks_with_contract(
        response,
        ResponsesModelBlocks::openai(),
        ResponsesContract::Official,
    );
    let mut output = None;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            ModelBlockStreamEvent::BlockStarted(block) => {
                assert_eq!(block.kind, ModelBlockKind::AssistantText)
            }
            ModelBlockStreamEvent::ReasoningCompleted { output: reasoning } => {
                output = Some(reasoning)
            }
            _ => {}
        }
    }
    let output = output.unwrap();
    assert_eq!(output.display_text(), "");
    assert!(
        matches!(output.continuation, Some(ReasoningContinuation::Blocks(blocks)) if blocks.len() == 2)
    );
}

#[tokio::test]
async fn responses_commits_phase_from_item_done_or_terminal_snapshot() {
    for phase_on_item_done in [false, true] {
        let added = serde_json::json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_1","type":"message","phase":null}});
        let delta = serde_json::json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg_1","delta":"Working"});
        let mut completed_item = serde_json::json!({"id":"msg_1","type":"message","content":[{"type":"output_text","text":"Working"}]});
        if phase_on_item_done {
            completed_item["phase"] = serde_json::json!("commentary");
        }
        let item_done = serde_json::json!({"type":"response.output_item.done","output_index":0,"item":completed_item});
        completed_item["phase"] = serde_json::json!("commentary");
        let terminal =
            serde_json::json!({"type":"response.completed","response":{"output":[completed_item]}});
        let body = [added, delta, item_done, terminal]
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        for contract in [ResponsesContract::Official, ResponsesContract::Compatible] {
            let response = reqwest::Response::from(http::Response::new(body.clone()));
            let mut stream =
                model_blocks_with_contract(response, ResponsesModelBlocks::openai(), contract);
            let Some(Ok(ModelBlockStreamEvent::BlockStarted(started))) = stream.next().await else {
                panic!("assistant block should start pending");
            };
            assert_eq!(started.output_phase, ModelOutputPhase::Unknown);
            assert!(matches!(
                stream.next().await.unwrap().unwrap(),
                ModelBlockStreamEvent::BlockDelta { .. }
            ));
            let mut observed = Vec::new();
            while let Some(event) = stream.next().await {
                match event.unwrap() {
                    ModelBlockStreamEvent::AssistantTextPhase {
                        model_item_id,
                        phase,
                    } => {
                        assert_eq!(model_item_id, started.model_item_id);
                        assert_eq!(phase, ModelOutputPhase::Commentary);
                        observed.push("phase");
                    }
                    ModelBlockStreamEvent::BlockCompleted(completed) => {
                        assert_eq!(completed.model_item_id, started.model_item_id);
                        observed.push("complete");
                    }
                    ModelBlockStreamEvent::Done { .. } => observed.push("done"),
                    other => panic!("unexpected event: {other:?}"),
                }
            }
            let expected = if phase_on_item_done {
                ["phase", "complete", "done"]
            } else {
                ["complete", "phase", "done"]
            };
            assert_eq!(observed, expected);
        }
    }
}
