use super::*;
use talos_llm_provider::{
    ChatMessage, FunctionCall, LlmProvider, ModelBlockStream, ModelBlockStreamEvent,
    ReasoningContinuation, ReasoningEffort,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_sse_server(body: String) -> (String, tokio::sync::oneshot::Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test server should bind");
    let address = listener
        .local_addr()
        .expect("test server address should exist");
    let (captured, request) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("request should arrive");
        let mut buffer = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let read = socket.read(&mut chunk).await.expect("request should read");
            assert!(read > 0, "request should not be empty");
            buffer.extend_from_slice(&chunk[..read]);
            let Some(header_end) = buffer.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                })
                .expect("content length should exist")
                .1
                .trim()
                .parse()
                .expect("content length should be valid");
            if buffer.len() < header_end + 4 + length {
                continue;
            }
            let value = serde_json::from_slice(&buffer[header_end + 4..header_end + 4 + length])
                .expect("request JSON should be valid");
            let _ = captured.send(value);
            break;
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("response should write");
    });
    (format!("http://{address}"), request)
}

#[tokio::test]
async fn light_effort_reaches_both_deepseek_transports_as_low() {
    for protocol in [DeepSeekProtocol::Chat, DeepSeekProtocol::Messages] {
        let (base_url, captured) = spawn_sse_server(String::new()).await;
        let provider = DeepSeekProvider::new(
            DeepSeekConfig::new("test-key")
                .with_base_url(base_url)
                .with_protocol(protocol),
        );
        provider
            .send_stream(
                ChatRequest::builder()
                    .model("deepseek-v4-flash-vision-exp")
                    .message("Inspect the page")
                    .reasoning_effort(Some(ReasoningEffort::Light))
                    .build(),
            )
            .await
            .expect("actual request should succeed");
        let actual = captured.await.expect("request should be captured");
        assert_eq!(actual["model"], "deepseek-v4-flash-vision-exp");
        assert_eq!(actual["thinking"]["type"], "enabled");
        match protocol {
            DeepSeekProtocol::Chat => assert_eq!(actual["reasoning_effort"], "low"),
            DeepSeekProtocol::Messages => assert_eq!(actual["output_config"]["effort"], "low"),
        }
    }
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
fn vision_chat_encodes_user_and_tool_images_without_changing_tool_identity() {
    let provider = DeepSeekProvider::new(DeepSeekConfig::new("test-key"));
    let image = Attachment::new(
        AttachmentKind::Image,
        "screenshot.png",
        "image/png",
        vec![1, 2, 3].into(),
    );
    let mut tool = ChatMessage::tool("capture-1", "screenshot", "Captured page");
    tool.attachments = Some(vec![image.clone()]);
    let request = ChatRequest::builder()
        .model("deepseek-v4-flash-vision-exp")
        .messages(vec![
            ChatMessage::user_with_attachments("Inspect this image", vec![image]).unwrap(),
            ChatMessage::assistant_with_tool_calls(
                "",
                vec![FunctionCall::new(
                    "capture-1",
                    "screenshot",
                    serde_json::json!({}),
                )],
            ),
            tool,
        ])
        .build();
    let (endpoint, body) = provider.request_body(request, true).unwrap();
    assert_eq!(endpoint, "chat/completions");
    let image_block = serde_json::json!({
        "type": "image_url", "image_url": {"url": "data:image/png;base64,AQID"}
    });
    assert_eq!(body["messages"][0]["content"][1], image_block);
    assert_eq!(body["messages"][2]["content"][1], image_block);
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["messages"][2]["tool_call_id"], "capture-1");
}

#[test]
fn chat_maps_supported_efforts_and_omits_disabled_reasoning() {
    for (effort, expected) in [
        (ReasoningEffort::Light, "low"),
        (ReasoningEffort::Medium, "high"),
        (ReasoningEffort::High, "high"),
    ] {
        let request = ChatRequest::builder()
            .model("deepseek-v4-pro")
            .message("hello")
            .temperature(0.7)
            .top_p(0.9)
            .reasoning_effort(Some(effort))
            .build();
        let actual =
            serde_json::to_value(DeepSeekProvider::build_chat_request(request, false).unwrap())
                .unwrap();
        assert_eq!(actual["thinking"]["type"], "enabled");
        assert_eq!(actual["reasoning_effort"], expected);
        assert!(actual.get("temperature").is_none());
        assert!(actual.get("top_p").is_none());
    }

    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .message("hello")
        .build();
    let actual =
        serde_json::to_value(DeepSeekProvider::build_chat_request(request, false).unwrap())
            .unwrap();
    assert!(actual.get("thinking").is_none());
    assert!(actual.get("reasoning_effort").is_none());
}

#[test]
fn chat_maps_xhigh_effort_to_max() {
    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .message("hello")
        .reasoning_effort(Some(ReasoningEffort::XHigh))
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
        .reasoning_effort(Some(ReasoningEffort::XHigh))
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
    let (base_url, _) = spawn_sse_server(body).await;
    let provider = DeepSeekProvider::new(
        DeepSeekConfig::new("test-key")
            .with_base_url(base_url)
            .with_default_model("deepseek-v4-pro"),
    );
    let request = ChatRequest::builder()
        .model("deepseek-v4-pro")
        .message("hello")
        .reasoning_effort(Some(ReasoningEffort::High))
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
