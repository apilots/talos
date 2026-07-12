use super::*;
use talos_error::TalosError;
use talos_llm_provider::{
    protocols::messages as messages_protocol, CacheControl, CacheControlKind, ChatMessage,
    ChatRequest, EffectiveReasoning, LlmProvider, ModelBlockDelta, ModelBlockKind,
    ModelBlockStream, ModelBlockStreamEvent, PromptCacheSupport, ReasoningContinuation,
    ReasoningEffort, ReasoningSupport, ToolDefinition,
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
fn test_anthropic_config_builder() {
    let config = AnthropicConfig::new("test-api-key")
        .with_base_url("https://custom.anthropic.com/v1")
        .with_timeout(120);

    assert_eq!(config.api_key, "test-api-key");
    assert_eq!(config.base_url, "https://custom.anthropic.com/v1");
    assert_eq!(config.timeout_secs, 120);
}

#[tokio::test]
async fn test_stream_model_blocks_maps_content_block_lifecycle() {
    let body = concat!(
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"think\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"signed\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"read\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"/tmp/a\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider =
        AnthropicProvider::new(AnthropicConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "claude-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "anthropic-block-0-thinking"
                && started.kind == ModelBlockKind::Reasoning
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
            if model_item_id == "anthropic-block-1-text" && delta == "hello"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "anthropic-block-2-tool"
                && started.provider_item_id.as_deref() == Some("call_1")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockCompleted(completed)
            if completed.model_item_id == "anthropic-block-2-tool"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::ReasoningCompleted { output }
            if output.display_text == vec!["think".to_string()]
                && matches!(output.continuation, Some(ReasoningContinuation::Blocks(ref blocks)) if blocks.len() == 1)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::Done { usage: Some(usage), .. }
            if usage.completion_tokens == 2
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_buffers_split_sse_lines() {
    let chunks = vec![
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n"
            .to_string(),
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"he".to_string(),
        "llo\"}}\n\n".to_string(),
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string(),
        "data: {\"type\":\"message_stop\"}\n\n".to_string(),
    ];
    let base_url = spawn_sse_server_chunks(chunks).await;
    let provider =
        AnthropicProvider::new(AnthropicConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "claude-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
            if model_item_id == "anthropic-block-0-text" && delta == "hello"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_fails_invalid_sse_json() {
    let base_url = spawn_sse_server("data: {not-json}\n\n".to_string()).await;
    let provider =
        AnthropicProvider::new(AnthropicConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "claude-test"))
        .await
        .expect("stream should start");

    let err = collect_first_model_error(stream).await;

    assert!(format!("{err}").contains("invalid SSE JSON"));
}

#[test]
fn test_convert_messages() {
    let messages = vec![
        ChatMessage::system("You are a helpful assistant"),
        ChatMessage::user("Hello"),
        ChatMessage::assistant("Hi there!"),
    ];

    let (system, anthropic_msgs) =
        messages_protocol::messages(messages).expect("messages should map");
    let first = serde_json::to_value(&anthropic_msgs[0]).expect("message should serialize");
    let second = serde_json::to_value(&anthropic_msgs[1]).expect("message should serialize");

    assert_eq!(system.as_ref().map(|blocks| blocks.len()), Some(1));
    assert_eq!(anthropic_msgs.len(), 2);
    assert_eq!(first["role"], "user");
    assert!(first["content"].is_array());
    assert_eq!(second["role"], "assistant");
    assert!(second["content"].is_array());
}

#[test]
fn test_convert_messages_with_tool_result() {
    let messages = vec![ChatMessage::tool("call_1", "read", "done")];
    let (_system, anthropic_msgs) =
        messages_protocol::messages(messages).expect("messages should map");
    let first = serde_json::to_value(&anthropic_msgs[0]).expect("message should serialize");
    assert_eq!(anthropic_msgs.len(), 1);
    assert_eq!(first["role"], "user");
    assert!(first["content"].is_array());
}

#[test]
fn test_provider_name() {
    let config = AnthropicConfig::new("test-key");
    let provider = AnthropicProvider::new(config);
    assert_eq!(provider.name(), "anthropic");
    assert_eq!(provider.default_model(), "claude-sonnet-4-6");
    assert_eq!(
        provider.prompt_cache_support(),
        PromptCacheSupport::RequestLevel
    );
}

#[test]
fn test_reasoning_support_is_model_gated() {
    let provider = AnthropicProvider::new(AnthropicConfig::new("test-key"));

    assert_eq!(
        provider.reasoning_support("claude-3-5-sonnet-20241022"),
        ReasoningSupport::Unsupported
    );
    assert_eq!(
        provider.reasoning_support("claude-3-7-sonnet-20250219"),
        ReasoningSupport::Supported
    );
    assert_eq!(
        provider.reasoning_support("claude-opus-4-8"),
        ReasoningSupport::Supported
    );
    assert_eq!(
        provider.reasoning_support("claude-sonnet-4-6"),
        ReasoningSupport::Supported
    );
    assert_eq!(
        provider.reasoning_support("claude-haiku-4-5-20251001"),
        ReasoningSupport::Supported
    );
}

#[test]
fn test_build_headers() {
    let config = AnthropicConfig::new("test-api-key");
    let provider = AnthropicProvider::new(config);
    let headers = provider.build_headers();

    assert!(headers.contains_key("x-api-key"));
    assert!(headers.contains_key("anthropic-version"));
    assert!(headers.contains_key("accept"));
    assert!(headers.contains_key("content-type"));
    assert_eq!(
        headers.get("x-talos-client").and_then(|v| v.to_str().ok()),
        Some("talos")
    );
    assert_eq!(
        headers.get("x-talos-version").and_then(|v| v.to_str().ok()),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        headers.get("user-agent").and_then(|v| v.to_str().ok()),
        Some(concat!("talos/", env!("CARGO_PKG_VERSION")))
    );
}

#[test]
fn test_build_headers_for_compatible_endpoint() {
    let config = AnthropicConfig::new("test-api-key").with_base_url("https://example.com/v1");
    let provider = AnthropicProvider::new(config);
    let headers = provider.build_headers();

    assert!(!headers.contains_key("x-api-key"));
    assert_eq!(
        headers.get("accept").and_then(|v| v.to_str().ok()),
        Some("*/*")
    );
    assert!(headers.contains_key("authorization"));
    assert_eq!(
        headers.get("authorization").and_then(|v| v.to_str().ok()),
        Some("Bearer test-api-key")
    );
    assert_eq!(
        headers.get("x-talos-client").and_then(|v| v.to_str().ok()),
        Some("talos")
    );
}

#[test]
fn test_convert_tools() {
    let tools = vec![ToolDefinition::new(
        "search",
        "Search docs",
        Some(serde_json::json!({"type": "object"})),
    )];
    let converted = messages_protocol::tools(Some(tools)).unwrap();
    let json = serde_json::to_value(&converted[0]).expect("tool should serialize");
    assert_eq!(converted.len(), 1);
    assert_eq!(json["name"], "search");
}

#[test]
fn test_build_request_maps_reasoning_budget() {
    let request = ChatRequest::builder()
        .model("claude-3-5-sonnet-20241022")
        .messages(vec![ChatMessage::user("hello")])
        .reasoning(EffectiveReasoning::Effort(ReasoningEffort::High))
        .build();

    let built = AnthropicProvider::build_request(request, None).expect("request should build");
    let json = serde_json::to_value(&built).expect("request should serialize");

    assert_eq!(json["thinking"]["budget_tokens"], 4096);
}

#[test]
fn test_convert_messages_preserves_multiple_system_blocks_and_cache_control() {
    let mut first = ChatMessage::system("System text");
    first.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let messages = vec![first, ChatMessage::system("Workspace context")];

    let (system_blocks, anthropic_msgs) =
        messages_protocol::messages(messages).expect("messages should map");
    let serialized = serde_json::to_value(system_blocks.expect("system blocks should exist"))
        .expect("system blocks should serialize");

    assert!(anthropic_msgs.is_empty());
    assert_eq!(serialized.as_array().map(|items| items.len()), Some(2));
    assert_eq!(serialized[0]["cache_control"]["type"], "ephemeral");
    assert_eq!(serialized[1]["text"], "Workspace context");
}

#[test]
fn test_text_block_serialization_skips_null_cache_control() {
    let block = messages_protocol::text_block("hello".to_string(), None);
    let json = serde_json::to_string(&block).expect("text block should serialize");
    assert!(!json.contains("cache_control"));
    assert!(!json.contains("null"));
}
