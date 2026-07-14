use super::*;
use talos_error::TalosError;
use talos_llm_provider::{
    protocols::chat as chat_protocol, CacheControl, CacheControlKind, ChatMessage, ChatRequest,
    LlmProvider, ModelBlockDelta, ModelBlockKind, ModelBlockStream, ModelBlockStreamEvent,
    PromptCacheSupport, ReasoningContinuation, ReasoningEffort,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_sse_server(body: String) -> String {
    spawn_sse_server_chunks(vec![body]).await
}

async fn spawn_sse_server_chunks(chunks: Vec<String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test server should bind");
    let addr = listener
        .local_addr()
        .expect("test server addr should exist");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("request should arrive");
        let mut buf = vec![0; 4096];
        let _ = socket.read(&mut buf).await.expect("request should read");
        let content_length: usize = chunks.iter().map(|chunk| chunk.len()).sum();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
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
    });
    format!("http://{addr}")
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
    assert_eq!(config.timeout_secs, 120);
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

#[test]
fn test_usage_from_bailian_maps_cache_fields() {
    let usage = chat_protocol::usage(chat_protocol::ChatCompletionUsage {
        prompt_tokens: 128,
        completion_tokens: 32,
        total_tokens: 160,
        prompt_tokens_details: Some(chat_protocol::ChatCompletionPromptTokensDetails {
            cached_tokens: Some(96),
        }),
        cache_creation: Some(chat_protocol::ChatCompletionCacheCreation {
            cache_creation_input_tokens: Some(48),
            ephemeral_5m_input_tokens: Some(48),
        }),
    })
    .expect("usage mapping should succeed");

    assert_eq!(usage.prompt_tokens, 128);
    assert_eq!(usage.completion_tokens, 32);
    assert_eq!(usage.total_tokens, 160);
    assert_eq!(usage.cache_read_input_tokens, Some(96));
    assert_eq!(usage.cache_creation_input_tokens, Some(48));
}

#[test]
fn test_usage_from_bailian_maps_ephemeral_cache_creation_fallback() {
    let usage = chat_protocol::usage(chat_protocol::ChatCompletionUsage {
        prompt_tokens: 256,
        completion_tokens: 64,
        total_tokens: 320,
        prompt_tokens_details: Some(chat_protocol::ChatCompletionPromptTokensDetails {
            cached_tokens: Some(128),
        }),
        cache_creation: Some(chat_protocol::ChatCompletionCacheCreation {
            cache_creation_input_tokens: None,
            ephemeral_5m_input_tokens: Some(72),
        }),
    })
    .expect("usage mapping should succeed");

    assert_eq!(usage.prompt_tokens, 256);
    assert_eq!(usage.completion_tokens, 64);
    assert_eq!(usage.total_tokens, 320);
    assert_eq!(usage.cache_read_input_tokens, Some(128));
    assert_eq!(usage.cache_creation_input_tokens, Some(72));
}

#[test]
fn test_usage_from_bailian_rejects_conflicting_cache_creation_fields() {
    let err = chat_protocol::usage(chat_protocol::ChatCompletionUsage {
        prompt_tokens: 256,
        completion_tokens: 64,
        total_tokens: 320,
        prompt_tokens_details: Some(chat_protocol::ChatCompletionPromptTokensDetails {
            cached_tokens: Some(128),
        }),
        cache_creation: Some(chat_protocol::ChatCompletionCacheCreation {
            cache_creation_input_tokens: Some(72),
            ephemeral_5m_input_tokens: Some(96),
        }),
    })
    .expect_err("usage mapping should reject conflicting cache creation fields");

    assert!(matches!(err, TalosError::ValidationError(_)));
}
