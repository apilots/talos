use super::*;
use talos_llm_provider::{
    ChatMessage, FunctionCall, LlmProvider, ReasoningContinuation, ReasoningEffort,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[test]
fn chat_maps_reasoning_effort_for_ollama() {
    for (effort, expected) in [
        (ReasoningEffort::Light, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::XHigh, "high"),
    ] {
        let request = ChatRequest::builder()
            .model("gpt-oss:20b")
            .message("hello")
            .reasoning_effort(Some(effort))
            .build();
        let actual =
            serde_json::to_value(OllamaProvider::build_request(request, false).unwrap()).unwrap();
        assert_eq!(actual["reasoning_effort"], expected);
    }

    let request = ChatRequest::builder()
        .model("gpt-oss:20b")
        .message("hello")
        .build();
    let actual =
        serde_json::to_value(OllamaProvider::build_request(request, false).unwrap()).unwrap();
    assert!(actual.get("reasoning_effort").is_none());
}

#[test]
fn chat_replays_reasoning_before_tool_calls() {
    let request = ChatRequest::builder()
        .model("gpt-oss:20b")
        .messages(vec![ChatMessage::assistant_with_tool_calls(
            "",
            vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
        )
        .with_reasoning_continuation(ReasoningContinuation::Text(
            "reasoning".to_string(),
        ))])
        .build();

    let request =
        OllamaProvider::build_request(request, false).expect("Ollama chat request should build");
    let actual = serde_json::to_value(request).expect("chat request should serialize");

    assert_eq!(actual["messages"][0]["reasoning"], "reasoning");
    assert_eq!(actual["messages"][0]["tool_calls"][0]["id"], "call-1");
}

#[tokio::test]
async fn chat_uses_ollama_v1_endpoint_and_identity() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test server should bind");
    let addr = listener
        .local_addr()
        .expect("test server address should exist");
    let (path_tx, path_rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("request should arrive");
        let mut buffer = vec![0; 4096];
        let read = socket.read(&mut buffer).await.expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..read]);
        let path = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or_default()
            .to_string();
        path_tx.send(path).expect("path should send");

        let body = serde_json::json!({
            "model": "qwen3",
            "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("response should write");
    });

    let provider = OllamaProvider::new(OllamaConfig::new(format!("http://{addr}"), "qwen3"));
    let response = provider
        .chat(ChatRequest::new_user("hello", "qwen3"))
        .await
        .expect("chat should succeed");

    assert_eq!(provider.name(), "ollama");
    assert_eq!(provider.default_model(), "qwen3");
    assert_eq!(
        path_rx.await.expect("path should be captured"),
        "/v1/chat/completions"
    );
    assert_eq!(response.content, "ok");
}
