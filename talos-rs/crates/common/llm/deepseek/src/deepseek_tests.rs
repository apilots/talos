use super::*;
use talos_llm_provider::{
    ChatMessage, EffectiveReasoning, FunctionCall, LlmProvider, ModelBlockStream,
    ModelBlockStreamEvent, ReasoningContinuation, ReasoningEffort,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_sse_server(body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test server should bind");
    let address = listener
        .local_addr()
        .expect("test server address should exist");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("request should arrive");
        let mut buffer = vec![0; 4096];
        let read = socket.read(&mut buffer).await.expect("request should read");
        assert!(read > 0, "request should not be empty");
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("response should write");
    });
    format!("http://{address}")
}

async fn collect_events(mut stream: ModelBlockStream) -> Vec<ModelBlockStreamEvent> {
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        let event = event.expect("stream event should succeed");
        let done = matches!(event, ModelBlockStreamEvent::Done { .. });
        events.push(event);
        if done {
            break;
        }
    }
    events
}

#[test]
fn chat_maps_low_effort_to_high_and_enables_thinking() {
    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .message("hello")
        .temperature(0.7)
        .top_p(0.9)
        .reasoning(EffectiveReasoning::Effort(ReasoningEffort::Low))
        .build();

    let request =
        DeepSeekProvider::build_chat_request(request, false).expect("chat request should build");
    let actual = serde_json::to_value(request).expect("chat request should serialize");

    assert_eq!(actual["thinking"]["type"], "enabled");
    assert_eq!(actual["reasoning_effort"], "high");
    assert!(actual.get("temperature").is_none());
    assert!(actual.get("top_p").is_none());
}

#[test]
fn chat_maps_xhigh_effort_to_max() {
    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .message("hello")
        .reasoning(EffectiveReasoning::Effort(ReasoningEffort::XHigh))
        .build();

    let request =
        DeepSeekProvider::build_chat_request(request, false).expect("chat request should build");
    let actual = serde_json::to_value(request).expect("chat request should serialize");

    assert_eq!(actual["reasoning_effort"], "max");
}

#[test]
fn messages_maps_effort_without_budget_tokens() {
    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .message("hello")
        .reasoning(EffectiveReasoning::Effort(ReasoningEffort::XHigh))
        .build();

    let request = DeepSeekProvider::build_messages_request(request, None)
        .expect("messages request should build");
    let actual = serde_json::to_value(request).expect("messages request should serialize");

    assert_eq!(actual["thinking"]["type"], "enabled");
    assert!(actual["thinking"].get("budget_tokens").is_none());
    assert_eq!(actual["output_config"]["effort"], "max");
}

#[test]
fn chat_replays_reasoning_content_before_tool_calls() {
    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .messages(vec![ChatMessage::assistant_with_tool_calls(
            "working",
            vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
        )
        .with_reasoning_continuation(ReasoningContinuation::Text(
            "reasoning".to_string(),
        ))])
        .build();

    let request =
        DeepSeekProvider::build_chat_request(request, false).expect("chat request should build");
    let actual = serde_json::to_value(request).expect("chat request should serialize");

    assert_eq!(actual["messages"][0]["reasoning_content"], "reasoning");
    assert_eq!(actual["messages"][0]["tool_calls"][0]["id"], "call-1");
}

#[tokio::test]
async fn chat_stream_reasoning_content_replays_after_tool_call() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read\",\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = DeepSeekProvider::new(
        DeepSeekConfig::new("test-key")
            .with_base_url(base_url)
            .with_default_model("deepseek-v4-pro"),
    );
    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .message("hello")
        .reasoning(EffectiveReasoning::Effort(ReasoningEffort::High))
        .build();

    let events = collect_events(
        provider
            .stream_model_blocks(request)
            .await
            .expect("stream should start"),
    )
    .await;
    let continuation = events
        .iter()
        .find_map(|event| match event {
            ModelBlockStreamEvent::ReasoningCompleted { output } => output.continuation.clone(),
            _ => None,
        })
        .expect("reasoning continuation should complete");
    let replay = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .messages(vec![ChatMessage::assistant_with_tool_calls(
            "",
            vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
        )
        .with_reasoning_continuation(continuation)])
        .build();
    let replay =
        DeepSeekProvider::build_chat_request(replay, false).expect("replay request should build");
    let actual = serde_json::to_value(replay).expect("replay request should serialize");

    assert_eq!(actual["messages"][0]["reasoning_content"], "think");
}
