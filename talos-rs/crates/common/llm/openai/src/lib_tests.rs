use super::*;
use talos_error::TalosError;
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
            file_id: None,
            kind: AttachmentKind::Image,
            filename: "fire.png".to_string(),
            media_type: "image/png".to_string(),
            bytes: vec![0_u8; 218 * 1024].into(),
        },
        Attachment {
            file_id: None,
            kind: AttachmentKind::File,
            filename: "report.docx".to_string(),
            media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                .to_string(),
            bytes: vec![1_u8; 16 * 1024].into(),
        },
        Attachment {
            file_id: None,
            kind: AttachmentKind::File,
            filename: "data.csv".to_string(),
            media_type: "text/csv".to_string(),
            bytes: b"a,b\n1,2\n".to_vec().into(),
        },
        Attachment {
            file_id: None,
            kind: AttachmentKind::File,
            filename: "sheet.xlsx".to_string(),
            media_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                .to_string(),
            bytes: vec![2_u8; 16 * 1024].into(),
        },
        Attachment {
            file_id: None,
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
fn test_responses_url_appends_endpoint_to_base_url() {
    let provider = OpenAIProvider::new(
        OpenAIConfig::new("test-api-key")
            .with_base_url("https://dashscope.aliyuncs.com/compatible-mode/v1/"),
    );

    assert_eq!(
        provider.responses_url(),
        "https://dashscope.aliyuncs.com/compatible-mode/v1/responses"
    );
}

#[tokio::test]
async fn test_stream_model_blocks_buffers_split_sse_lines() {
    let chunks = vec![
        r#"data: {"type":"response.output_text.delta","output_index":0,"delta":"he"#.to_string(),
        "llo\"}\n\n".to_string(),
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n".to_string(),
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
                if model_item_id == "openai-response-0-text" && delta == "hello"
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
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"/tmp/a\\\"}\"}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":3,\"item\":{\"type\":\"message\",\"id\":\"msg_2\",\"phase\":\"final_answer\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":3,\"delta\":\"hello\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
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
fn test_responses_request_maps_reasoning_effort() {
    let provider = OpenAIProvider::new(OpenAIConfig::new(""));
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
        let built = provider.responses_request(request, None).unwrap();
        assert_eq!(built.reasoning.unwrap().effort, expected);
    }

    let request = ChatRequest::builder()
        .model("gpt-5")
        .message("hello")
        .build();
    let built = provider.responses_request(request, None).unwrap();
    assert!(built.reasoning.is_none());
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
    let provider = OpenAIProvider::new(OpenAIConfig::new(""));
    let mut msg = ChatMessage::user("Hello");
    msg.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let request = provider
        .responses_request(
            ChatRequest::builder()
                .model("gpt-4o")
                .messages(vec![msg])
                .build(),
            None,
        )
        .expect("request should build");

    let json = serde_json::to_string(&request).expect("request should serialize");
    assert!(!json.contains("cache_control"));
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
fn official_responses_requests_stateless_reasoning_and_preserves_optional_tool_fields() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {"note": {"type": "string"}},
        "required": [],
        "additionalProperties": false
    });
    let continuation =
        ReasoningContinuation::Blocks(vec![talos_llm_provider::ReasoningBlock::Encrypted(
            talos_llm_provider::ReasoningEncryptedBlock {
                provider_item_id: Some("rs_1".to_owned()),
                provider_index: None,
                data: "opaque".to_owned(),
                format: None,
            },
        )]);
    let request = ChatRequest::builder()
        .model("gpt-5.4-mini")
        .messages(vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("").with_reasoning_continuation(continuation),
        ])
        .tools(vec![
            ToolDefinition::new("example", "Optional note", Some(schema.clone())),
            ToolDefinition::new("no_args", "No arguments", None),
        ])
        .build();
    let provider = OpenAIProvider::new(OpenAIConfig::new("").with_base_url("http://127.0.0.1/v1"));
    let body =
        serde_json::to_value(provider.responses_request(request, Some(true)).unwrap()).unwrap();
    assert_eq!(body["store"], false);
    assert_eq!(body["input"][1]["summary"], serde_json::json!([]));
    assert!(body.get("reasoning").is_none());
    assert_eq!(
        body["include"],
        serde_json::json!(["reasoning.encrypted_content"])
    );
    assert_eq!(body["tools"][0]["parameters"], schema);
    assert_eq!(body["tools"][0]["strict"], false);
    assert_eq!(
        body["tools"][1].get("parameters"),
        Some(&serde_json::Value::Null)
    );
}

#[test]
fn official_responses_requests_summary_only_with_enabled_reasoning() {
    let provider = OpenAIProvider::new(
        OpenAIConfig::new("").with_base_url("https://openai-gateway.example/v1"),
    );
    let request = ChatRequest::builder()
        .model("gpt-6-luna")
        .messages(vec![ChatMessage::user("hello")])
        .reasoning_effort(Some(ReasoningEffort::Medium))
        .build();

    for stream in [Some(true), None] {
        let body =
            serde_json::to_value(provider.responses_request(request.clone(), stream).unwrap())
                .unwrap();
        assert_eq!(
            body["reasoning"],
            serde_json::json!({"effort": "medium", "summary": "auto"})
        );
    }

    let off = ChatRequest::new_user("hello", "gpt-6-luna");
    let body = serde_json::to_value(provider.responses_request(off, Some(true)).unwrap()).unwrap();
    assert_eq!(body["reasoning"], serde_json::json!({"effort": "none"}));
}

#[test]
fn official_reasoning_off_distinguishes_gpt6_sol_from_gpt61_sol() {
    let provider = OpenAIProvider::new(OpenAIConfig::new(""));
    for (model, expected) in [
        ("gpt-6-sol", Some("none")),
        ("gpt-6.1-sol", None),
        ("gpt-6-luna", Some("none")),
        ("gpt-6.1-terra", Some("none")),
        ("gpt-6.2-astra", None),
        ("gpt-6.2-sol", None),
    ] {
        let request = ChatRequest::new_user("hello", model);
        let responses = provider.responses_request(request, Some(true)).unwrap();
        assert_eq!(
            responses
                .reasoning
                .as_ref()
                .map(|reasoning| reasoning.effort.as_str()),
            expected,
            "{model}"
        );
    }
}

#[path = "request_retry_tests.rs"]
mod request_retry_tests;
