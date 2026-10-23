use super::*;
use talos_llm_provider::{
    Attachment, AttachmentKind, CacheControl, CacheControlKind, ChatMessage, ModelBlockDelta,
    ModelBlockKind, ModelBlockStreamEvent, ModelOutputPhase, PromptCacheSupport,
    ReasoningContinuation, ReasoningEffort, ToolDefinition,
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
fn test_openai_config_builder() {
    let config = OpenAIConfig::new("test-api-key")
        .with_base_url("https://custom.openai.com/v1")
        .with_timeout(120);

    assert_eq!(config.api_key, "test-api-key");
    assert_eq!(config.base_url, "https://custom.openai.com/v1");
    assert_eq!(config.timeout_secs, 120);
    assert_eq!(config.protocol, OpenAIProtocol::Chat);
}

#[test]
fn test_openai_config_can_select_responses_protocol() {
    let config = OpenAIConfig::new("test-api-key").with_protocol(OpenAIProtocol::Responses);

    assert_eq!(config.protocol, OpenAIProtocol::Responses);
}

#[test]
fn test_openai_config_can_set_default_model() {
    let config = OpenAIConfig::new("test-api-key").with_default_model("gpt-5.5-mini");

    assert_eq!(config.default_model, "gpt-5.5-mini");
}

#[test]
fn openai_accepts_normal_sized_images_and_configured_file_types() {
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-api-key"));
    let attachments = vec![
        Attachment {
            kind: AttachmentKind::Image,
            filename: "fire.png".to_string(),
            media_type: "image/png".to_string(),
            bytes: vec![0_u8; 218 * 1024].into(),
        },
        Attachment {
            kind: AttachmentKind::File,
            filename: "report.docx".to_string(),
            media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                .to_string(),
            bytes: vec![1_u8; 16 * 1024].into(),
        },
        Attachment {
            kind: AttachmentKind::File,
            filename: "data.csv".to_string(),
            media_type: "text/csv".to_string(),
            bytes: b"a,b\n1,2\n".to_vec().into(),
        },
        Attachment {
            kind: AttachmentKind::File,
            filename: "sheet.xlsx".to_string(),
            media_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                .to_string(),
            bytes: vec![2_u8; 16 * 1024].into(),
        },
        Attachment {
            kind: AttachmentKind::File,
            filename: "paper.pdf".to_string(),
            media_type: "application/pdf".to_string(),
            bytes: b"%PDF-1.7".to_vec().into(),
        },
    ];

    let usage = provider
        .validate_attachments("gpt-5.5", &attachments)
        .expect("OpenAI inline attachments should validate");

    assert_eq!(usage.item_tokens, vec![10_000; attachments.len()]);
}

#[test]
fn test_endpoint_url_appends_protocol_endpoint_to_base_url() {
    let provider = OpenAIProvider::new(
        OpenAIConfig::new("test-api-key")
            .with_base_url("https://dashscope.aliyuncs.com/compatible-mode/v1/"),
    );

    assert_eq!(
        provider.endpoint_url("chat/completions"),
        "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
    );
    assert_eq!(
        provider.endpoint_url("responses"),
        "https://dashscope.aliyuncs.com/compatible-mode/v1/responses"
    );
}

#[tokio::test]
async fn test_stream_model_blocks_maps_chat_completion_chunks() {
    let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"think\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"/tmp/a\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "openai-choice-0-reasoning"
                && started.kind == ModelBlockKind::Reasoning
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "openai-choice-0-assistant"
                && started.output_phase == ModelOutputPhase::Unknown
    )));
    assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
                if model_item_id == "openai-choice-0-assistant" && delta == "hello"
        )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "openai-tool-0"
                && started.provider_item_id.as_deref() == Some("call_1")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockCompleted(completed)
            if completed.model_item_id == "openai-tool-0"
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
async fn test_stream_model_blocks_buffers_split_sse_lines() {
    let chunks = vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he".to_string(),
        "llo\"}}]}\n\n".to_string(),
        "data: [DONE]\n\n".to_string(),
    ];
    let base_url = spawn_sse_server_chunks(chunks).await;
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
                if model_item_id == "openai-choice-0-assistant" && delta == "hello"
        )));
}

#[tokio::test]
async fn test_stream_model_blocks_maps_responses_events() {
    let body = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\"}}\n\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"output_index\":0,\"delta\":\"think\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"think\"}],\"encrypted_content\":\"encrypted\"}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"phase\":\"commentary\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"delta\":\"working\"}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"read\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\"{\\\"path\\\"\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\":\\\"/tmp/a\\\"}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"read\"}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":3,\"item\":{\"type\":\"message\",\"id\":\"msg_2\",\"phase\":\"final_answer\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":3,\"delta\":\"hello\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        )
        .to_string();
    let base_url = spawn_sse_server(body).await;
    let provider = OpenAIProvider::new(
        OpenAIConfig::new("test-api-key")
            .with_base_url(base_url)
            .with_protocol(OpenAIProtocol::Responses),
    );
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
        .await
        .expect("stream should start");

    let events = collect_model_events(stream).await;

    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "openai-response-0-reasoning"
                && started.provider_item_id.as_deref() == Some("rs_1")
                && started.kind == ModelBlockKind::Reasoning
    )));
    assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::ReasoningText { delta } }
                if model_item_id == "openai-response-0-reasoning" && delta == "think"
        )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "openai-response-1-text"
                && started.provider_item_id.as_deref() == Some("msg_1")
                && started.kind == ModelBlockKind::AssistantText
                && started.output_phase == ModelOutputPhase::Commentary
    )));
    assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
                if model_item_id == "openai-response-1-text" && delta == "working"
        )));
    assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockStarted(started)
                if started.model_item_id == "openai-response-2-tool"
                    && started.provider_item_id.as_deref() == Some("fc_1")
                    && started.tool_call.as_ref().and_then(|call| call.call_id.as_deref()) == Some("call_1")
                    && started.tool_call.as_ref().and_then(|call| call.tool_name.as_deref()) == Some("read")
        )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockStarted(started)
            if started.model_item_id == "openai-response-3-text"
                && started.provider_item_id.as_deref() == Some("msg_2")
                && started.kind == ModelBlockKind::AssistantText
                && started.output_phase == ModelOutputPhase::FinalAnswer
    )));
    assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
                if model_item_id == "openai-response-3-text" && delta == "hello"
        )));
    assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::ToolArguments { delta } }
                if model_item_id == "openai-response-2-tool" && delta == "{\"path\""
        )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::BlockCompleted(completed)
            if completed.model_item_id == "openai-response-2-tool"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::ReasoningCompleted { output }
            if output.display_text == vec!["think".to_string()]
                && matches!(output.continuation, Some(ReasoningContinuation::Blocks(ref blocks)) if blocks.len() == 2)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelBlockStreamEvent::Done { usage: Some(usage), finish_reason: Some(reason) }
            if usage.total_tokens == 3 && reason == "stop"
    )));
}

#[tokio::test]
async fn test_stream_model_blocks_fails_invalid_sse_json() {
    let base_url = spawn_sse_server("data: {not-json}\n\n".to_string()).await;
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-api-key").with_base_url(base_url));
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
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

    let openai_msgs = chat_protocol::messages(
        messages,
        false,
        chat_protocol::ChatReasoningContinuationFormat::Reasoning,
    )
    .expect("messages should map");

    assert_eq!(openai_msgs.len(), 3);
    assert_eq!(openai_msgs[0].role, "system");
    assert!(matches!(
        openai_msgs[0].content.as_ref(),
        Some(chat_protocol::ChatCompletionMessageContent::Text(text))
            if text == "You are a helpful assistant"
    ));
    assert_eq!(openai_msgs[1].role, "user");
    assert!(matches!(
        openai_msgs[1].content.as_ref(),
        Some(chat_protocol::ChatCompletionMessageContent::Text(text)) if text == "Hello"
    ));
    assert_eq!(openai_msgs[2].role, "assistant");
    assert!(matches!(
        openai_msgs[2].content.as_ref(),
        Some(chat_protocol::ChatCompletionMessageContent::Text(text)) if text == "Hi there!"
    ));
}

#[test]
fn test_openai_request_serialization_with_tools() {
    let req = chat_protocol::request(
        ChatRequest::builder()
            .model("gpt-4o")
            .messages(vec![ChatMessage::user("Hello")])
            .temperature(0.7)
            .max_tokens(100)
            .tools(vec![ToolDefinition::new(
                "read",
                "Read file",
                Some(serde_json::json!({"type": "object"})),
            )])
            .tool_choice(talos_llm_provider::ToolChoice::Auto)
            .parallel_tool_calls(true)
            .build(),
        false,
        chat_protocol::ChatCompletionRequestOptions::default(),
    )
    .expect("request should build");

    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"model\":\"gpt-4o\""));
    assert!(json.contains("\"tools\""));
    assert!(json.contains("\"tool_choice\":\"auto\""));
}

#[test]
fn test_build_request_maps_reasoning_effort() {
    for (effort, expected) in [
        (ReasoningEffort::Light, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::XHigh, "xhigh"),
    ] {
        let request = ChatRequest::builder()
            .model("gpt-5")
            .message("hello")
            .reasoning_effort(Some(effort))
            .build();
        let built = OpenAIProvider::build_request(request, false).unwrap();
        assert_eq!(built.reasoning_effort.as_deref(), Some(expected));
    }

    let request = ChatRequest::builder()
        .model("gpt-5")
        .message("hello")
        .build();
    let built = OpenAIProvider::build_request(request, false).unwrap();
    assert_eq!(built.reasoning_effort, None);
}

#[test]
fn test_parse_tool_calls() {
    let calls = vec![chat_protocol::ChatCompletionToolCall {
        id: "call_1".to_string(),
        r#type: "function".to_string(),
        function: chat_protocol::ChatCompletionFunctionCall {
            name: "read".to_string(),
            arguments: "{\"path\":\"/tmp/a\"}".to_string(),
        },
    }];

    let parsed = chat_protocol::tool_calls(Some(calls)).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].name, "read");
    assert_eq!(parsed[0].arguments["path"], "/tmp/a");
}

#[test]
fn test_provider_name() {
    let config = OpenAIConfig::new("test-key");
    let provider = OpenAIProvider::new(config);
    assert_eq!(provider.name(), "openai");
    assert_eq!(provider.default_model(), "gpt-5.5");
    assert_eq!(
        provider.prompt_cache_support(),
        PromptCacheSupport::Unsupported
    );
}

#[test]
fn test_openai_request_does_not_serialize_cache_control_for_unsupported_provider() {
    let mut msg = ChatMessage::user("Hello");
    msg.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let request = OpenAIProvider::build_request(
        ChatRequest::builder()
            .model("gpt-4o")
            .messages(vec![msg])
            .build(),
        false,
    )
    .expect("request should build");

    let json = serde_json::to_string(&request).expect("request should serialize");
    assert!(!json.contains("cache_control"));
}

#[test]
fn test_extract_reasoning_delta_ignores_bailian_reasoning_content() {
    let delta = serde_json::json!({
        "reasoning_content": "bailian-only"
    });
    assert!(chat_protocol::reasoning_delta(&delta, &["reasoning"]).is_none());
}

#[test]
fn test_build_headers() {
    let config = OpenAIConfig::new("test-api-key");
    let provider = OpenAIProvider::new(config);
    let headers = provider.build_headers();

    assert!(headers.contains_key("authorization"));
    assert!(headers.contains_key("content-type"));

    let auth_header = headers.get("authorization").unwrap();
    assert_eq!(auth_header.to_str().unwrap(), "Bearer test-api-key");
    assert_eq!(
        headers.get("accept").and_then(|value| value.to_str().ok()),
        Some("*/*")
    );
    assert_eq!(
        headers
            .get("x-talos-client")
            .and_then(|value| value.to_str().ok()),
        Some("talos")
    );
    assert_eq!(
        headers
            .get("x-talos-version")
            .and_then(|value| value.to_str().ok()),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok()),
        Some(concat!("talos/", env!("CARGO_PKG_VERSION")))
    );
}

#[test]
fn test_convert_tools() {
    let defs = vec![ToolDefinition::new(
        "read",
        "Read file",
        Some(serde_json::json!({"type":"object"})),
    )];
    let converted = chat_protocol::tools(Some(defs)).unwrap();
    assert_eq!(converted.len(), 1);
    assert_eq!(converted[0].function.name, "read");
}

#[test]
fn configured_boolean_thinking_omits_effort_but_preserves_tool_reasoning_replay() {
    let mut request = ChatRequest::new_user("Continue", "model");
    request.reasoning_effort = Some(ReasoningEffort::High);
    request.request_parameters.insert(
        "chat_template_kwargs".to_owned(),
        serde_json::json!({"enable_thinking":true}),
    );
    let mut message = ChatMessage::assistant_with_tool_calls(
        "",
        vec![talos_llm_provider::FunctionCall::new(
            "call-1",
            "echo",
            serde_json::json!({"text":"hello"}),
        )],
    );
    message.reasoning_continuation =
        Some(ReasoningContinuation::Text("Call echo first".to_owned()));
    request.messages.push(message);
    request
        .messages
        .push(ChatMessage::tool("call-1", "echo", "hello"));
    let parameters = request.request_parameters.clone();
    let body = talos_llm_provider::serialize_with_parameters(
        OpenAIProvider::build_request(request, false).unwrap(),
        &parameters,
    )
    .unwrap();
    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(
        body["chat_template_kwargs"],
        serde_json::json!({"enable_thinking":true})
    );
    assert_eq!(body["messages"][1]["reasoning"], "Call echo first");
    assert_eq!(body["messages"][2]["tool_call_id"], "call-1");
}

#[tokio::test]
async fn openai_compatible_stream_retains_reasoning_content_continuation() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"Think first\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let base_url = spawn_sse_server(body.to_owned()).await;
    let provider = OpenAIProvider::new(OpenAIConfig::new("").with_base_url(base_url));
    let events = collect_model_events(
        provider
            .stream_model_blocks(ChatRequest::new_user("Hi", "model"))
            .await
            .unwrap(),
    )
    .await;
    assert!(events.iter().any(|event| matches!(event, ModelBlockStreamEvent::ReasoningCompleted { output } if output.continuation == Some(ReasoningContinuation::Text("Think first".into())))));
}

#[test]
fn compatible_chat_merges_initial_system_instructions_without_reordering_history() {
    let request = ChatRequest::builder()
        .model("model")
        .messages(vec![
            ChatMessage::system("First policy"),
            ChatMessage::system("Second policy"),
            ChatMessage::user("Question"),
            ChatMessage::assistant("Answer"),
            ChatMessage::system("Later boundary"),
            ChatMessage::user("Follow up"),
        ])
        .build();
    let encoded =
        serde_json::to_value(OpenAIProvider::build_request(request, true).unwrap()).unwrap();
    assert_eq!(
        encoded["messages"],
        serde_json::json!([
            {"role":"system","content":"First policy\n\nSecond policy"},
            {"role":"user","content":"Question"},
            {"role":"assistant","content":"Answer"},
            {"role":"system","content":"Later boundary"},
            {"role":"user","content":"Follow up"},
        ])
    );
}
