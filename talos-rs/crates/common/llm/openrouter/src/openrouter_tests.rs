use super::*;
use talos_llm_provider::{
    Attachment, AttachmentKind, ChatMessage, FunctionCall, LlmProvider, ModelBlockStream,
    ModelBlockStreamEvent, ReasoningBlock, ReasoningContinuation, ReasoningEffort,
    ReasoningTextBlock,
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

fn reasoning_request(effort: ReasoningEffort) -> ChatRequest {
    ChatRequest::builder()
        .model("provider/model")
        .message("hello")
        .reasoning_effort(Some(effort))
        .build()
}

#[test]
fn chat_maps_reasoning_effort_to_openrouter_object() {
    for (effort, expected) in [
        (ReasoningEffort::Light, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::XHigh, "xhigh"),
    ] {
        let request = OpenRouterProvider::build_chat_request(reasoning_request(effort), false)
            .expect("chat request should build");
        let actual = serde_json::to_value(request).unwrap();
        assert_eq!(actual["reasoning"]["effort"], expected);
    }

    let request = ChatRequest::builder()
        .model("provider/model")
        .message("hello")
        .build();
    let actual =
        serde_json::to_value(OpenRouterProvider::build_chat_request(request, false).unwrap())
            .unwrap();
    assert!(actual.get("reasoning").is_none());
}

#[test]
fn responses_maps_reasoning_effort_and_encrypted_continuation() {
    let request = OpenRouterProvider::build_responses_request(
        reasoning_request(ReasoningEffort::Medium),
        None,
    )
    .expect("responses request should build");
    let actual = serde_json::to_value(request).expect("responses request should serialize");

    assert_eq!(actual["reasoning"]["effort"], "medium");
    assert_eq!(actual["include"][0], "reasoning.encrypted_content");
}

#[test]
fn messages_maps_reasoning_effort_to_output_config() {
    let request =
        OpenRouterProvider::build_messages_request(reasoning_request(ReasoningEffort::Light), None)
            .expect("messages request should build");
    let actual = serde_json::to_value(request).expect("messages request should serialize");

    assert_eq!(actual["output_config"]["effort"], "low");
}

#[test]
fn openrouter_applies_file_media_policy_for_the_selected_protocol() {
    let docx = Attachment {
        kind: AttachmentKind::File,
        filename: "report.docx".to_string(),
        media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            .to_string(),
        bytes: vec![0_u8; 16 * 1024].into(),
    };
    let responses = OpenRouterProvider::new(
        OpenRouterConfig::new("test-api-key").with_protocol(OpenRouterProtocol::Responses),
    );
    let messages = OpenRouterProvider::new(
        OpenRouterConfig::new("test-api-key").with_protocol(OpenRouterProtocol::Messages),
    );

    let usage = responses
        .validate_attachments("provider/model", std::slice::from_ref(&docx))
        .expect("Responses inline files should accept DOCX");

    assert_eq!(usage.item_tokens, vec![10_000]);
    assert!(messages
        .validate_attachments("provider/model", &[docx])
        .is_err());
}

#[test]
fn chat_replays_structured_reasoning_before_tool_calls() {
    let continuation =
        ReasoningContinuation::Blocks(vec![ReasoningBlock::Text(ReasoningTextBlock {
            provider_item_id: Some("reasoning-1".to_string()),
            provider_index: Some(0),
            text: "reasoning".to_string(),
            signature: Some("signature".to_string()),
            format: Some("anthropic-claude-v1".to_string()),
        })]);
    let request = ChatRequest::builder()
        .model("provider/model")
        .messages(vec![ChatMessage::assistant_with_tool_calls(
            "",
            vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
        )
        .with_reasoning_continuation(continuation)])
        .build();

    let request =
        OpenRouterProvider::build_chat_request(request, false).expect("chat request should build");
    let actual = serde_json::to_value(request).expect("chat request should serialize");

    assert_eq!(actual["messages"][0]["reasoning_details"][0]["index"], 0);
    assert_eq!(actual["messages"][0]["tool_calls"][0]["id"], "call-1");
}

#[tokio::test]
async fn chat_stream_reasoning_details_replay_after_tool_call() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"think\",\"id\":\"reasoning-1\",\"format\":\"openrouter-v1\",\"index\":0}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read\",\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = OpenRouterProvider::new(
        OpenRouterConfig::new("test-key")
            .with_base_url(base_url)
            .with_default_model("provider/model"),
    );

    let events = collect_events(
        provider
            .stream_model_blocks(reasoning_request(ReasoningEffort::High))
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
        .model("provider/model")
        .messages(vec![ChatMessage::assistant_with_tool_calls(
            "",
            vec![FunctionCall::new("call-1", "read", serde_json::json!({}))],
        )
        .with_reasoning_continuation(continuation)])
        .build();
    let replay =
        OpenRouterProvider::build_chat_request(replay, false).expect("replay request should build");
    let actual = serde_json::to_value(replay).expect("replay request should serialize");

    assert_eq!(
        actual["messages"][0]["reasoning_details"][0]["text"],
        "think"
    );
    assert_eq!(actual["messages"][0]["reasoning_details"][0]["index"], 0);
}
