use super::*;
use talos_llm_provider::{
    ChatMessage, FunctionCall, LlmProvider, ModelBlockDelta, ModelBlockKind, ModelBlockStream,
    ModelBlockStreamEvent, ReasoningContinuation, ReasoningEffort, ReasoningOutput, ToolDefinition,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_sse_server(body: String) -> (String, tokio::sync::oneshot::Receiver<Value>) {
    spawn_response_server(body, "text/event-stream").await
}

async fn spawn_response_server(
    body: String,
    content_type: &'static str,
) -> (String, tokio::sync::oneshot::Receiver<Value>) {
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
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
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

fn argument_fidelity_request(model: &str) -> ChatRequest {
    ChatRequest::builder()
        .model(model)
        .messages(vec![
            ChatMessage::system("Apply existing tool policies."),
            ChatMessage::system("<prompt-region>previous: first &amp;&amp; second</prompt-region>"),
            ChatMessage::user(
                "Call Bash with printf '%s' 'A&amp;B &amp;&amp;' && printf '%s' '<tag>'",
            ),
        ])
        .tools(vec![ToolDefinition::new(
            "Bash",
            "Run an approved command",
            Some(serde_json::json!({
                "type": "object", "properties": {"command": {"type": "string"}},
                "required": ["command"]
            })),
        )])
        .reasoning_effort(Some(ReasoningEffort::High))
        .build()
}

#[tokio::test]
async fn flash_tool_guidance_reaches_both_transports_without_rewriting_context() {
    for model in [
        "deepseek-flash",
        "deepseek-v4-flash",
        "deepseek-v4-flash-vision-exp",
    ] {
        for protocol in [DeepSeekProtocol::Chat, DeepSeekProtocol::Messages] {
            let request = argument_fidelity_request(model);
            let expected = match protocol {
                DeepSeekProtocol::Chat => serde_json::to_value(
                    DeepSeekProvider::build_chat_request(request.clone(), true).unwrap(),
                ),
                DeepSeekProtocol::Messages => serde_json::to_value(
                    DeepSeekProvider::build_messages_request(request.clone(), Some(true)).unwrap(),
                ),
            }
            .unwrap();
            let (base_url, captured) = spawn_sse_server(String::new()).await;
            let provider = DeepSeekProvider::new(
                DeepSeekConfig::new("test-key")
                    .with_base_url(base_url)
                    .with_protocol(protocol),
            );
            provider.send_stream(request).await.unwrap();
            let mut actual = captured.await.unwrap();
            let guidance = match protocol {
                DeepSeekProtocol::Chat => actual["messages"].as_array_mut().unwrap().remove(2),
                DeepSeekProtocol::Messages => actual["system"].as_array_mut().unwrap().remove(2),
            };
            let expected_guidance = match protocol {
                DeepSeekProtocol::Chat => serde_json::json!({
                    "role": "system", "content": TOOL_ARGUMENT_FIDELITY_GUIDANCE
                }),
                DeepSeekProtocol::Messages => serde_json::json!({
                    "type": "text", "text": TOOL_ARGUMENT_FIDELITY_GUIDANCE
                }),
            };
            assert_eq!(guidance, expected_guidance);
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn guidance_leaves_other_models_and_requests_without_tools_unchanged() {
    let mut without_tools = argument_fidelity_request("deepseek-flash");
    without_tools.tools = None;
    let mut empty_tools = without_tools.clone();
    empty_tools.tools = Some(Vec::new());
    for request in [
        without_tools,
        empty_tools,
        argument_fidelity_request("deepseek-v4-pro"),
        argument_fidelity_request("custom-model"),
    ] {
        for protocol in [DeepSeekProtocol::Chat, DeepSeekProtocol::Messages] {
            let provider =
                DeepSeekProvider::new(DeepSeekConfig::new("test-key").with_protocol(protocol));
            let (_, actual) = provider.request_body(request.clone(), true).unwrap();
            let expected = match protocol {
                DeepSeekProtocol::Chat => serde_json::to_value(
                    DeepSeekProvider::build_chat_request(request.clone(), true).unwrap(),
                ),
                DeepSeekProtocol::Messages => serde_json::to_value(
                    DeepSeekProvider::build_messages_request(request.clone(), Some(true)).unwrap(),
                ),
            }
            .unwrap();
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn final_binding_includes_provider_guidance_without_changing_or_duplicating_wire_input() {
    for protocol in [DeepSeekProtocol::Chat, DeepSeekProtocol::Messages] {
        let provider =
            DeepSeekProvider::new(DeepSeekConfig::new("test-key").with_protocol(protocol));
        let mut request = argument_fidelity_request("deepseek-v4-flash");
        request.temperature = Some(0.25);
        request.top_p = Some(0.8);
        let (_, expected) = provider.request_body(request.clone(), true).unwrap();
        let bound = provider.bind_request(request.clone()).unwrap();
        assert_eq!(bound.messages.len(), request.messages.len() + 1);
        assert_eq!(bound.messages[2].content, TOOL_ARGUMENT_FIDELITY_GUIDANCE);
        assert_eq!(
            bound.messages[2].input_origin,
            Some(MessageInputOrigin {
                kind: MessageInputKind::System,
                source: MessageInputSource::Runtime,
            })
        );
        assert_eq!(bound.messages[3], request.messages[2]);
        if protocol == DeepSeekProtocol::Chat {
            assert_eq!(bound.temperature, None);
            assert_eq!(bound.top_p, None);
        }
        let rebound = provider.bind_request(bound).unwrap();
        let (_, actual) = provider.request_body(rebound, true).unwrap();
        assert_eq!(actual, expected);
    }
}

#[tokio::test]
async fn chat_response_preserves_empty_or_nonempty_reasoning_and_literal_tool_arguments() {
    let arguments = serde_json::json!({
        "command": "printf '%s' 'A&amp;B &amp;&amp;' && printf '%s' '<tag>'"
    });
    for reasoning in ["", "Use the exact requested command."] {
        let body = serde_json::json!({
            "model": "deepseek-flash",
            "choices": [{
                "message": {
                    "role": "assistant", "content": "", "reasoning_content": reasoning,
                    "tool_calls": [{
                        "id": "call-1", "type": "function",
                        "function": {"name": "Bash", "arguments": arguments.to_string()}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
        });
        let (base_url, _) = spawn_response_server(body.to_string(), "application/json").await;
        let provider =
            DeepSeekProvider::new(DeepSeekConfig::new("test-key").with_base_url(base_url));
        let response = provider
            .chat_with_tools(argument_fidelity_request("deepseek-flash"))
            .await
            .unwrap();
        let expected_reasoning = if reasoning.is_empty() {
            ReasoningOutput::default()
        } else {
            ReasoningOutput {
                display_text: vec![reasoning.to_string()],
                continuation: Some(ReasoningContinuation::Text(reasoning.to_string())),
            }
        };
        assert_eq!(response.reasoning, expected_reasoning);
        assert_eq!(response.content, "");
        assert_eq!(response.model, "deepseek-flash");
        assert_eq!(response.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            response.tool_calls,
            Some(vec![FunctionCall::new("call-1", "Bash", arguments.clone())])
        );
        assert_eq!(response.usage.total_tokens, 3);
    }
}

#[tokio::test]
async fn empty_reasoning_stream_completes_without_rewriting_tool_arguments() {
    let arguments = serde_json::json!({
        "command": "printf '%s' 'A&amp;B &amp;&amp;' && printf '%s' '<tag>'"
    })
    .to_string();
    let (first, second) = arguments.split_at(arguments.len() / 2);
    for protocol in [DeepSeekProtocol::Chat, DeepSeekProtocol::Messages] {
        let (chunks, stop_reason) = match protocol {
            DeepSeekProtocol::Chat => (
                vec![
                    serde_json::json!({"choices": [{"index": 0, "delta": {
                        "reasoning_content": "", "tool_calls": [{"index": 0, "id": "call-1",
                            "function": {"name": "Bash", "arguments": first}}]
                    }}]}),
                    serde_json::json!({"choices": [{"index": 0, "delta": {
                        "tool_calls": [{"index": 0, "function": {"arguments": second}}]
                    }}]}),
                    serde_json::json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
                        "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}}),
                ],
                "tool_calls",
            ),
            DeepSeekProtocol::Messages => (
                vec![
                    serde_json::json!({"type": "message_start", "message": {
                        "usage": {"input_tokens": 1, "output_tokens": 0}
                    }}),
                    serde_json::json!({"type": "content_block_start", "index": 0,
                        "content_block": {"type": "tool_use", "id": "call-1", "name": "Bash", "input": {}}}),
                    serde_json::json!({"type": "content_block_delta", "index": 0,
                        "delta": {"type": "input_json_delta", "partial_json": first}}),
                    serde_json::json!({"type": "content_block_delta", "index": 0,
                        "delta": {"type": "input_json_delta", "partial_json": second}}),
                    serde_json::json!({"type": "content_block_stop", "index": 0}),
                    serde_json::json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"},
                        "usage": {"output_tokens": 2}}),
                    serde_json::json!({"type": "message_stop"}),
                ],
                "tool_use",
            ),
        };
        let mut body = chunks
            .iter()
            .map(|chunk| format!("data: {chunk}\n\n"))
            .collect::<String>();
        if protocol == DeepSeekProtocol::Chat {
            body.push_str("data: [DONE]\n\n");
        }
        let (base_url, _) = spawn_sse_server(body).await;
        let provider = DeepSeekProvider::new(
            DeepSeekConfig::new("test-key")
                .with_base_url(base_url)
                .with_protocol(protocol),
        );
        let events = collect_events(
            provider
                .stream_model_blocks(argument_fidelity_request("deepseek-flash"))
                .await
                .unwrap(),
        )
        .await;
        let actual_arguments = events
            .iter()
            .filter_map(|event| match event {
                ModelBlockStreamEvent::BlockDelta {
                    delta: ModelBlockDelta::ToolArguments { delta },
                    ..
                } => Some(delta.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(actual_arguments, arguments);
        assert!(!events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockStarted(talos_llm_provider::ModelBlockStarted {
                kind: ModelBlockKind::Reasoning,
                ..
            }) | ModelBlockStreamEvent::ReasoningCompleted { .. }
        )));
        let (finish_reason, usage) = events
            .iter()
            .find_map(|event| match event {
                ModelBlockStreamEvent::Done {
                    finish_reason,
                    usage,
                } => Some((finish_reason.as_deref(), usage.as_ref())),
                _ => None,
            })
            .expect("empty reasoning must not prevent terminal completion");
        assert_eq!(finish_reason, Some(stop_reason));
        assert_eq!(usage.unwrap().total_tokens, 3);
    }
}
