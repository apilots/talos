use super::*;
use talos_error::{ErrorCode, TalosError};
use talos_llm_provider::{
    protocols::chat as chat_protocol, Attachment, AttachmentKind, CacheControl, CacheControlKind,
    ChatMessage, ChatRequest, LlmProvider, ModelBlockDelta, ModelBlockKind, ModelBlockStream,
    ModelBlockStreamEvent, PromptCacheSupport, ReasoningContinuation, ReasoningEffort,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_sse_server(body: String) -> String {
    spawn_sse_server_chunks(vec![body]).await
}

async fn spawn_sse_server_chunks(chunks: Vec<String>) -> String {
    spawn_http_response("text/event-stream", chunks).await
}

async fn spawn_http_response(content_type: &'static str, chunks: Vec<String>) -> String {
    spawn_http_exchange(content_type, chunks).await.0
}

async fn spawn_http_exchange(
    content_type: &'static str,
    chunks: Vec<String>,
) -> (String, tokio::task::JoinHandle<(String, serde_json::Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test server should bind");
    let addr = listener
        .local_addr()
        .expect("test server addr should exist");
    let request = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("request should arrive");
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let body_offset = loop {
            let count = socket.read(&mut buffer).await.expect("request should read");
            assert_ne!(count, 0, "request ended before its headers");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let mut headers = std::str::from_utf8(&bytes[..body_offset]).unwrap().lines();
        let request_line = headers.next().unwrap().to_owned();
        let body_length: usize = headers
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length").then_some(value)
            })
            .expect("request should have content length")
            .trim()
            .parse()
            .unwrap();
        while bytes.len() < body_offset + body_length {
            let count = socket
                .read(&mut buffer)
                .await
                .expect("request body should read");
            assert_ne!(count, 0, "request ended before its body");
            bytes.extend_from_slice(&buffer[..count]);
        }
        let body = serde_json::from_slice(&bytes[body_offset..body_offset + body_length]).unwrap();
        let content_length: usize = chunks.iter().map(|chunk| chunk.len()).sum();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            content_length
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("headers should write");
        for chunk in chunks {
            socket
                .write_all(chunk.as_bytes())
                .await
                .expect("chunk should write");
        }
        (request_line, body)
    });
    (format!("http://{addr}"), request)
}

async fn collect_model_events(mut stream: ModelBlockStream) -> Vec<ModelBlockStreamEvent> {
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        let event = event.expect("model block event should succeed");
        let done = matches!(event, ModelBlockStreamEvent::Done { .. });
        events.push(event);
        if done {
            break;
        }
    }
    events
}

async fn collect_first_model_error(mut stream: ModelBlockStream) -> TalosError {
    while let Some(event) = stream.next().await {
        if let Err(err) = event {
            return err;
        }
    }
    panic!("stream should emit an error");
}

#[test]
fn test_bailian_config_builder() {
    let config = BailianConfig::new("test-api-key")
        .with_base_url("https://example.com/compatible-mode/v1")
        .with_timeout(120);

    assert_eq!(config.api_key, "test-api-key");
    assert_eq!(config.base_url, "https://example.com/compatible-mode/v1");
}

#[test]
fn test_bailian_attachment_validation_accepts_images_and_rejects_files() {
    let provider = BailianProvider::new(BailianConfig::new("test-key"));
    let image = Attachment {
        file_id: None,
        kind: AttachmentKind::Image,
        filename: "pixel.png".to_string(),
        media_type: "image/png".to_string(),
        bytes: vec![1, 2, 3].into(),
    };

    assert!(provider
        .validate_attachments("qwen3.7-plus", std::slice::from_ref(&image))
        .is_ok());
    assert!(provider
        .validate_attachments(
            "qwen3.7-plus",
            &[Attachment {
                file_id: None,
                kind: AttachmentKind::File,
                filename: "note.pdf".to_string(),
                media_type: "application/pdf".to_string(),
                bytes: vec![1, 2, 3].into(),
            }]
        )
        .is_err_and(|error| error.is_code(ErrorCode::UnsupportedAttachment)));
}

#[tokio::test]
async fn test_stream_model_blocks_maps_chat_completion_chunks() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"/tmp/a\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "bailian-choice-0-reasoning"
                && started.kind == ModelBlockKind::Reasoning
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
            if model_item_id == "bailian-choice-0-assistant" && delta == "hello"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "bailian-tool-0"
                && started.provider_item_id.as_deref() == Some("call_1")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockCompleted(completed)
            if completed.model_item_id == "bailian-tool-0"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::ReasoningCompleted { output }
            if output.display_text == vec!["think".to_string()]
                && output.continuation == Some(ReasoningContinuation::Text("think".to_string()))
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::Done { usage: Some(usage), .. }
            if usage.total_tokens == 3
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_waits_for_tool_name_before_starting_tool_block() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\"}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"Read\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"README.md\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "bailian-tool-0"
                && started.provider_item_id.as_deref() == Some("call_1")
                && started
                    .tool_call
                    .as_ref()
                    .and_then(|tool_call| tool_call.tool_name.as_deref())
                    == Some("Read")
    )));
    let tool_arguments = events.iter().fold(String::new(), |mut arguments, event| {
        match event {
            ModelBlockStreamEvent::BlockDelta {
                model_item_id,
                delta: ModelBlockDelta::ToolArguments { delta },
            } if model_item_id == "bailian-tool-0" => arguments.push_str(delta),
            _ => {}
        }
        arguments
    });
    assert_eq!(tool_arguments, "{\"path\":\"README.md\"}");
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockCompleted(completed)
            if completed.model_item_id == "bailian-tool-0"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_preserves_tool_calls_with_empty_text_fields() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\",\"role\":\"assistant\",\"reasoning_content\":\"\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"Read\",\"arguments\":\"\"}}],\"content\":\"\",\"reasoning_content\":\"\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"type\":\"function\",\"function\":{\"arguments\":\"{\\\"path\\\": \\\"\"}}],\"content\":\"\",\"reasoning_content\":\"\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"type\":\"function\",\"function\":{\"arguments\":\"README.md\"}}],\"content\":\"\",\"reasoning_content\":\"\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"type\":\"function\",\"function\":{\"arguments\":\"\\\"}\"}}],\"content\":\"\",\"reasoning_content\":\"\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":null}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let request = ChatRequest::builder()
        .model("qwen3.7-plus")
        .messages(vec![ChatMessage::user("read README")])
        .tools(vec![talos_llm_provider::ToolDefinition::new(
            "Read",
            "Read a file",
            Some(serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"}
                }
            })),
        )])
        .build();
    let stream = provider
        .stream_model_blocks(request)
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "bailian-tool-0"
                && started.provider_item_id.as_deref() == Some("call_1")
                && started
                    .tool_call
                    .as_ref()
                    .and_then(|tool_call| tool_call.tool_name.as_deref())
                    == Some("Read")
    )));
    let tool_arguments = events.iter().fold(String::new(), |mut arguments, event| {
        match event {
            ModelBlockStreamEvent::BlockDelta {
                model_item_id,
                delta: ModelBlockDelta::ToolArguments { delta },
            } if model_item_id == "bailian-tool-0" => arguments.push_str(delta),
            _ => {}
        }
        arguments
    });
    assert_eq!(tool_arguments, "{\"path\": \"README.md\"}");
}

#[tokio::test]
async fn test_stream_model_blocks_segments_assistant_text_after_tool_boundary() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"before\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"after\"}}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockCompleted(completed)
            if completed.model_item_id == "bailian-choice-0-assistant"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
            if model_item_id == "bailian-choice-0-assistant-1" && delta == "after"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_segments_reasoning_after_tool_boundary() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"before\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"after\"}}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockCompleted(completed)
            if completed.model_item_id == "bailian-choice-0-reasoning"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::ReasoningText { delta } }
            if model_item_id == "bailian-choice-0-reasoning-1" && delta == "after"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_ignores_empty_text_chunks() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"\\n\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"visible\"}}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                ModelBlockStreamEvent::BlockStarted(started)
                    if started.kind == ModelBlockKind::AssistantText
            ))
            .count(),
        1
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.kind == ModelBlockKind::Reasoning
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta { delta: ModelBlockDelta::AssistantText { delta }, .. }
            if delta == "visible"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_ignores_reasoning_after_assistant_text() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"final\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"late\"}}]}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(!events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta {
            delta: ModelBlockDelta::ReasoningText { delta },
            ..
        } if delta == "late"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_buffers_split_sse_lines() {
    let chunks = vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he".to_string(),
        "llo\"}}]}\n\n".to_string(),
        "data: [DONE]\n\n".to_string(),
    ];
    let base_url = spawn_sse_server_chunks(chunks).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
            if model_item_id == "bailian-choice-0-assistant" && delta == "hello"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_fails_invalid_sse_json() {
    let base_url = spawn_sse_server("data: {not-json}\n\n".to_string()).await;
    let provider = BailianProvider::new(BailianConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "qwen-test"))
        .await
        .expect("stream should start");

    let err = collect_first_model_error(stream).await;

    assert!(format!("{err}").contains("invalid SSE JSON"));
}

#[test]
fn test_build_request_maps_reasoning_to_bailian_fields() {
    for (effort, budget) in [
        (ReasoningEffort::Light, 1024),
        (ReasoningEffort::Medium, 2048),
        (ReasoningEffort::High, 4096),
        (ReasoningEffort::XHigh, 8192),
    ] {
        let request = ChatRequest::builder()
            .model("qwen3.7-plus")
            .message("hello")
            .reasoning_effort(Some(effort))
            .build();
        let json =
            serde_json::to_value(BailianProvider::build_request(request, true).unwrap()).unwrap();
        assert_eq!(json["enable_thinking"], true);
        assert_eq!(json["thinking_budget"], budget);
    }

    let request = ChatRequest::builder()
        .model("qwen")
        .message("hello")
        .build();
    let json =
        serde_json::to_value(BailianProvider::build_request(request, false).unwrap()).unwrap();
    assert!(json.get("enable_thinking").is_none());
    assert!(json.get("thinking_budget").is_none());

    let mut request = ChatRequest::new_user("hello", "qwen3.7-plus");
    request.max_tokens = Some(120);
    let encoded = BailianProvider::build_request(request, false).unwrap();
    assert_eq!(encoded.enable_thinking, Some(false));
    assert_eq!(encoded.thinking_budget, None);
    assert_eq!(encoded.max_tokens, Some(120));
    assert_eq!(encoded.max_completion_tokens, None);
}

#[test]
fn bailian_builds_documented_qwen_effort_and_output_limit() {
    for (model, effort, expected) in [
        (
            "qwen4-max-preview",
            Some(ReasoningEffort::High),
            Some("xhigh"),
        ),
        ("qwen3.8-max", Some(ReasoningEffort::Medium), Some("medium")),
        ("qwen3.8-flash", Some(ReasoningEffort::Light), Some("low")),
        ("qwen3.8-max-0902", None, Some("none")),
    ] {
        let mut request = ChatRequest::new_user("hello", model);
        request.reasoning_effort = effort;
        request.max_tokens = Some(120);
        let encoded = BailianProvider::build_request(request, true).unwrap();
        assert_eq!(encoded.reasoning_effort.as_deref(), expected, "{model}");
        assert_eq!(encoded.enable_thinking, None, "{model}");
        assert_eq!(encoded.thinking_budget, None, "{model}");
        assert_eq!(encoded.max_tokens, None, "{model}");
        assert_eq!(encoded.max_completion_tokens, Some(120), "{model}");
    }

    let request = ChatRequest::new_user("hello", "qwen3.8-2.4t-a95b");
    let encoded = BailianProvider::build_request(request, false).unwrap();
    assert_eq!(encoded.reasoning_effort, None);
    assert_eq!(encoded.enable_thinking, None);
}

#[tokio::test]
async fn bailian_sends_current_request_fields_in_both_response_modes() {
    for (stream, content_type, response) in [
        (
            false,
            "application/json",
            r#"{"model":"qwen3.8-max","choices":[{"message":{"content":"OK"},"finish_reason":"stop"}]}"#,
        ),
        (true, "text/event-stream", "data: [DONE]\n\n"),
    ] {
        let (base_url, exchange) =
            spawn_http_exchange(content_type, vec![response.to_owned()]).await;
        let config =
            BailianConfig::new("").with_base_url(format!("{base_url}/compatible-mode/v1/"));
        let provider = BailianProvider::new(config);
        let mut request = ChatRequest::new_user("hello", "qwen3.8-max");
        request.max_tokens = Some(120);

        if stream {
            let events =
                collect_model_events(provider.stream_model_blocks(request).await.unwrap()).await;
            assert!(matches!(
                events.as_slice(),
                [ModelBlockStreamEvent::Done { .. }]
            ));
        } else {
            assert_eq!(provider.chat(request).await.unwrap().content, "OK");
        }

        let (request_line, body) = exchange.await.unwrap();
        assert_eq!(
            request_line,
            "POST /compatible-mode/v1/chat/completions HTTP/1.1"
        );
        assert_eq!(body["stream"], stream);
        assert_eq!(body["reasoning_effort"], "none");
        assert_eq!(body["max_completion_tokens"], 120);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("thinking_budget").is_none());
    }
}

#[test]
fn bailian_respects_explicit_budget_and_rejects_conflicting_effort() {
    let mut request = ChatRequest::new_user("hello", "qwen3.8-max");
    request.reasoning_effort = Some(ReasoningEffort::Medium);
    request.max_tokens = Some(120);
    request
        .request_parameters
        .insert("thinking_budget".into(), serde_json::json!(512));
    request
        .request_parameters
        .insert("max_tokens".into(), serde_json::json!(90));

    let encoded = BailianProvider::build_request(request.clone(), false).unwrap();
    let body = talos_llm_provider::serialize_with_parameters(encoded, &request.request_parameters)
        .unwrap();
    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(body["enable_thinking"], true);
    assert_eq!(body["thinking_budget"], 512);
    assert_eq!(body["max_tokens"], 90);
    assert!(body.get("max_completion_tokens").is_none());

    request
        .request_parameters
        .insert("reasoning_effort".into(), serde_json::json!("low"));
    let error = BailianProvider::build_request(request.clone(), false).unwrap_err();
    assert!(error.is_code(ErrorCode::InvalidRequest));

    request.request_parameters.remove("thinking_budget");
    let encoded = BailianProvider::build_request(request.clone(), false).unwrap();
    let body = talos_llm_provider::serialize_with_parameters(encoded, &request.request_parameters)
        .unwrap();
    assert_eq!(body["reasoning_effort"], "low");
    assert!(body.get("thinking_budget").is_none());
}

#[test]
fn test_extract_reasoning_delta_reads_reasoning_content() {
    let delta = serde_json::json!({
        "reasoning_content": [
            {"text": "first "},
            {"content": "second"}
        ]
    });
    assert_eq!(
        chat_protocol::reasoning_delta(&delta, &["reasoning_content", "thinking", "reasoning"])
            .as_deref(),
        Some("first second")
    );
}

#[test]
fn test_bailian_request_serializes_cache_control_for_request_level_provider() {
    let provider = BailianProvider::new(BailianConfig::new("test-key"));
    assert_eq!(
        provider.prompt_cache_support(),
        PromptCacheSupport::RequestLevel
    );

    let mut message = ChatMessage::user("hello");
    message.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let request = BailianProvider::build_request(
        ChatRequest::builder()
            .model("qwen3.7-plus")
            .messages(vec![message])
            .build(),
        false,
    )
    .expect("request should build");

    let json = serde_json::to_string(&request).expect("request should serialize");
    assert!(json.contains("cache_control"));
}

#[test]
fn test_bailian_request_serializes_cache_control_as_content_block() {
    let mut message = ChatMessage::system("stable context");
    message.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let request = BailianProvider::build_request(
        ChatRequest::builder()
            .model("qwen3.7-plus")
            .messages(vec![message])
            .build(),
        false,
    )
    .expect("request should build");

    let json = serde_json::to_value(&request).expect("request should serialize");
    assert_eq!(json["messages"][0]["content"][0]["text"], "stable context");
    assert_eq!(
        json["messages"][0]["content"][0]["cache_control"]["type"],
        "ephemeral"
    );
}

#[tokio::test]
async fn bailian_decodes_its_cache_fields_in_json_and_sse() {
    use serde_json::json;
    use talos_error::ErrorCode;

    let cases = [
        (
            json!({"prompt_tokens_details":{"cache_creation_input_tokens":40}}),
            Ok(Some(40)),
        ),
        (
            json!({"cache_creation":{"cache_creation_input_tokens":48,"ephemeral_5m_input_tokens":48}}),
            Ok(Some(48)),
        ),
        (
            json!({"cache_creation":{"ephemeral_5m_input_tokens":72}}),
            Ok(Some(72)),
        ),
        (
            json!({"prompt_tokens_details":{"cache_write_tokens":0,"cache_creation_input_tokens":40},
                "cache_creation":{"ephemeral_5m_input_tokens":72}}),
            Ok(Some(0)),
        ),
        (
            json!({"prompt_tokens_details":{},"cache_creation":null}),
            Ok(None),
        ),
        (
            json!({"cache_creation":{"cache_creation_input_tokens":72,"ephemeral_5m_input_tokens":96}}),
            Err(ErrorCode::InvalidRequest),
        ),
    ];
    for (mut wire_usage, expected) in cases {
        wire_usage["prompt_tokens"] = json!(128);
        wire_usage["completion_tokens"] = json!(32);
        wire_usage["total_tokens"] = json!(160);
        wire_usage["prompt_tokens_details"]["cached_tokens"] = json!(32);

        let non_streaming_body = json!({
            "model":"qwen-test",
            "choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],
            "usage":wire_usage,
        });
        let base_url =
            spawn_http_response("application/json", vec![non_streaming_body.to_string()]).await;
        let provider = BailianProvider::new(BailianConfig::new("").with_base_url(base_url));
        let request = ChatRequest::new_user("test", "qwen-test");
        let response_usage = provider
            .chat(request.clone())
            .await
            .map(|response| response.usage);

        let usage_chunk = json!({"choices":[],"usage":wire_usage});
        let sse = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"OK\"}},\"finish_reason\":\"stop\"}}]}}\n\ndata: {usage_chunk}\n\ndata: [DONE]\n\n"
        );
        let base_url = spawn_sse_server(sse).await;
        let provider = BailianProvider::new(BailianConfig::new("").with_base_url(base_url));
        let mut stream = provider.stream_model_blocks(request).await.unwrap();
        let stream_usage = loop {
            match stream
                .next()
                .await
                .expect("stream must emit a terminal event")
            {
                Ok(ModelBlockStreamEvent::Done { usage, .. }) => break Ok(usage.unwrap()),
                Ok(_) => {}
                Err(error) => break Err(error),
            }
        };

        for result in [response_usage, stream_usage] {
            let mapped = result
                .map(|usage| {
                    assert_eq!(usage.prompt_tokens, 128);
                    assert_eq!(usage.completion_tokens, 32);
                    assert_eq!(usage.total_tokens, 160);
                    assert_eq!(usage.cache_read_input_tokens, Some(32));
                    usage.cache_creation_input_tokens
                })
                .map_err(|error| error.error_code());
            assert_eq!(mapped, expected, "{wire_usage}");
        }
    }
}
