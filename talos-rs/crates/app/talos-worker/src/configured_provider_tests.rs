use std::time::Duration;
use talos_config::Config;
use talos_llm_provider::{ChatRequest, ModelBlockDelta, ModelBlockStreamEvent, ReasoningEffort};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn read_request(socket: &mut TcpStream) -> (String, serde_json::Value) {
    let mut bytes = Vec::new();
    let body_start = loop {
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk).await.unwrap();
        assert_ne!(read, 0);
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8(bytes[..body_start].to_vec()).unwrap();
    let length: usize = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() < body_start + length {
        let mut chunk = [0_u8; 4096];
        let read = socket.read(&mut chunk).await.unwrap();
        assert_ne!(read, 0);
        bytes.extend_from_slice(&chunk[..read]);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&bytes[body_start..body_start + length]).unwrap();
    (headers, body)
}

#[tokio::test]
async fn custom_protocol_sends_session_thinking_controls_without_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..6 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (headers, body) = read_request(&mut socket).await;
            requests.push((headers, body));
            let body = r#"{"model":"model","choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
            let response = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });
    let mut config = Config::default();
    config.llm = serde_json::from_value(serde_json::json!({
        "provider":"llm-gateway", "model":"model", "custom_providers":{
            "llm-gateway":{"name":"LLM Gateway","protocol":"chat","base_url":url,"models":[{"id":"model","reasoning":true,"request_parameters":{"chat_template_kwargs":{"enable_thinking":true,"other":7}}}]}
        }
    })).unwrap();
    for (efforts, static_effort) in [
        (None, None),
        (Some(vec![]), None),
        (
            Some(vec![ReasoningEffort::Light, ReasoningEffort::Medium]),
            Some("light"),
        ),
    ] {
        let model = &mut config
            .llm
            .custom_providers
            .get_mut("llm-gateway")
            .unwrap()
            .config
            .models[0];
        model.reasoning_efforts = efforts;
        if let Some(effort) = static_effort {
            model
                .request_parameters
                .insert("reasoning_effort".into(), serde_json::json!(effort));
        }
        config.llm.validate_providers().unwrap();
        let provider = crate::create_llm_provider(&config).unwrap();
        assert_eq!(provider.name(), "llm-gateway");
        if static_effort.is_some() {
            let mut invalid = ChatRequest::new_user("unsupported", "model");
            invalid.reasoning_effort = Some(ReasoningEffort::High);
            assert!(provider
                .bind_request(invalid)
                .unwrap_err()
                .is_code(talos_error::ErrorCode::InvalidRequest));
        }
        provider
            .chat(ChatRequest::new_user("Off", "model"))
            .await
            .unwrap();
        let mut enabled = ChatRequest::new_user("On", "model");
        enabled.reasoning_effort = Some(ReasoningEffort::Medium);
        let bound = provider.bind_request(enabled).unwrap();
        let repeated = provider.bind_request(bound.clone()).unwrap();
        assert_eq!(bound.reasoning_effort, repeated.reasoning_effort);
        assert_eq!(bound.request_parameters, repeated.request_parameters);
        provider.chat(repeated).await.unwrap();
    }
    let requests = server.await.unwrap();
    for ((headers, body), (thinking, effort)) in requests.into_iter().zip([
        (false, None),
        (true, Some("medium")),
        (false, None),
        (true, None),
        (false, Some("light")),
        (true, Some("light")),
    ]) {
        assert!(headers.starts_with("POST /v1/chat/completions"));
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        assert_eq!(body["model"], "model");
        assert_eq!(
            body["chat_template_kwargs"],
            serde_json::json!({"enable_thinking":thinking,"other":7})
        );
        assert_eq!(
            body.get("reasoning_effort"),
            effort.map(serde_json::Value::from).as_ref()
        );
    }
}

const NATIVE_CONNECTIONS: &[(&str, &str)] = &[
    ("openai", "responses"),
    ("anthropic", "messages"),
    ("bailian", "chat"),
    ("deepseek", "messages"),
    ("openrouter", "responses"),
    ("ollama", "chat"),
];

fn native_config(provider: &str, protocol: &str, url: &str) -> Config {
    let mut config = Config::default();
    config.llm = serde_json::from_value(serde_json::json!({
        "provider":provider, "model":"model",
        (provider): {"protocol":protocol, "base_url":url, "api_key":"loopback-only-key",
            "http_timeouts":{"connect_secs":1,"response_secs":1,"stream_idle_secs":1,"stream_total_secs":3},
            "models":[{"id":"model"}]}
    })).unwrap();
    config
}

async fn serve_response(
    status: &str,
    content_type: &'static str,
    header_delay: Duration,
    chunks: Vec<(Duration, &'static str)>,
) -> (String, tokio::task::JoinHandle<(String, serde_json::Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let status = status.to_owned();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        assert!(request.0.starts_with("POST "));
        tokio::time::sleep(header_delay).await;
        let headers = format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\nconnection: close\r\n\r\n"
        );
        if socket.write_all(headers.as_bytes()).await.is_err() {
            return request;
        }
        for (delay, chunk) in chunks {
            tokio::time::sleep(delay).await;
            if socket.write_all(chunk.as_bytes()).await.is_err() {
                return request;
            }
        }
        request
    });
    (url, task)
}

#[tokio::test]
async fn native_connections_allow_progress_beyond_nonstream_deadline() {
    for &(provider, protocol) in NATIVE_CONNECTIONS {
        let terminal = match protocol {
            "responses" => concat!(
                "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"OK\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
            ),
            "messages" => concat!(
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n",
                "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            ),
            _ => "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        };
        let (url, task) = serve_response(
            "200 OK",
            "text/event-stream",
            Duration::ZERO,
            vec![
                (Duration::from_millis(600), ": heartbeat\n\n"),
                (Duration::from_millis(600), terminal),
            ],
        )
        .await;
        let config = native_config(provider, protocol, &url);
        let provider = crate::create_llm_provider(&config).unwrap();
        let mut events = provider
            .stream_model_blocks(ChatRequest::new_user("hello", "model"))
            .await
            .unwrap();
        let mut done = false;
        let mut text = String::new();
        while let Some(event) = events.next().await {
            match event.unwrap() {
                ModelBlockStreamEvent::Done { .. } => done = true,
                ModelBlockStreamEvent::BlockDelta {
                    delta: ModelBlockDelta::AssistantText { delta },
                    ..
                } => text.push_str(&delta),
                _ => {}
            }
        }
        assert!(done);
        assert_eq!(text, "OK");
        task.await.unwrap();
    }
}

#[tokio::test]
async fn native_connections_keep_request_body_and_idle_timeouts_typed() {
    for &(provider, protocol) in NATIVE_CONNECTIONS {
        for (header_delay, stream) in [
            (Duration::from_secs(2), false),
            (Duration::ZERO, false),
            (Duration::ZERO, true),
        ] {
            let (url, task) = serve_response(
                "200 OK",
                "text/event-stream",
                header_delay,
                vec![(Duration::from_secs(2), "{}")],
            )
            .await;
            let config = native_config(provider, protocol, &url);
            let provider = crate::create_llm_provider(&config).unwrap();
            let request = ChatRequest::new_user("hello", "model");
            let error = tokio::time::timeout(Duration::from_secs(3), async {
                if stream {
                    provider
                        .stream_model_blocks(request)
                        .await
                        .unwrap()
                        .next()
                        .await
                        .unwrap()
                        .unwrap_err()
                } else {
                    provider.chat(request).await.unwrap_err()
                }
            })
            .await
            .unwrap();
            assert_eq!(error.error_code(), talos_error::ErrorCode::Timeout);
            task.abort();
        }
    }
}

#[tokio::test]
async fn native_connections_classify_upstream_http_rejections() {
    for &(provider, protocol) in NATIVE_CONNECTIONS {
        let (url, task) = serve_response("400 Bad Request", "application/json", Duration::ZERO, vec![
            (Duration::ZERO, r#"{"error":{"code":"context_length_exceeded","message":"private upstream detail"}}"#),
        ]).await;
        let config = native_config(provider, protocol, &url);
        let provider = crate::create_llm_provider(&config).unwrap();
        let error = provider
            .chat(ChatRequest::new_user("hello", "model"))
            .await
            .unwrap_err();
        assert_eq!(
            error.error_code(),
            talos_error::ErrorCode::ContextWindowExceeded
        );
        task.await.unwrap();
    }
}

#[tokio::test]
async fn native_maximum_selections_survive_configuration_and_request_binding() {
    use talos_types::{ModelRef, ModelSelection};
    for (provider_id, protocol, model_id, effort, expected) in [
        (
            "anthropic",
            "messages",
            "claude-opus-5-5",
            ReasoningEffort::Max,
            "max",
        ),
        (
            "anthropic",
            "messages",
            "claude-opus-5",
            ReasoningEffort::XHigh,
            "xhigh",
        ),
        (
            "bailian",
            "chat",
            "qwen4-max",
            ReasoningEffort::High,
            "high",
        ),
        (
            "bailian",
            "chat",
            "qwen4-max",
            ReasoningEffort::XHigh,
            "xhigh",
        ),
        (
            "deepseek",
            "chat",
            "deepseek-flash",
            ReasoningEffort::Max,
            "max",
        ),
        (
            "openrouter",
            "chat",
            "openai/gpt-6.2-sol",
            ReasoningEffort::Max,
            "max",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            let body = if protocol == "messages" {
                serde_json::json!({"id":"msg","type":"message","role":"assistant","model":model_id,"content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}})
            } else {
                serde_json::json!({"model":model_id,"choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
            }.to_string();
            let response = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
            request
        });
        let mut config = native_config(provider_id, protocol, &url);
        config.llm.model = model_id.to_owned();
        let selected = match provider_id {
            "anthropic" => &mut config.llm.anthropic,
            "bailian" => &mut config.llm.bailian,
            "deepseek" => &mut config.llm.deepseek,
            "openrouter" => &mut config.llm.openrouter,
            _ => unreachable!(),
        };
        let model = &mut selected.models[0];
        model.id = model_id.to_owned();
        model.reasoning = true;
        model.reasoning_efforts = Some(vec![ReasoningEffort::Medium, effort]);
        if provider_id == "anthropic" {
            model.request_parameters =
                serde_json::from_value(serde_json::json!({"thinking":{"type":"adaptive"}}))
                    .unwrap();
        }
        let selection = ModelSelection::new(
            ModelRef {
                provider: provider_id.to_owned(),
                id: model_id.to_owned(),
            },
            Some(effort),
        );
        let restored: ModelSelection =
            serde_json::from_value(serde_json::to_value(&selection).unwrap()).unwrap();
        let provider = crate::create_llm_provider(&config).unwrap();
        let mut request = ChatRequest::new_user("selected effort", model_id);
        request.reasoning_effort = restored.reasoning_effort;
        let bound = provider.bind_request(request).unwrap();
        let repeated = provider.bind_request(bound).unwrap();
        assert_eq!(repeated.reasoning_effort, Some(effort));
        provider.chat(repeated).await.unwrap();
        let (headers, body) = server.await.unwrap();
        if protocol == "messages" {
            assert!(headers.starts_with("POST /v1/messages"));
            assert_eq!(body["thinking"], serde_json::json!({"type":"adaptive"}));
            assert_eq!(
                body["output_config"],
                serde_json::json!({"effort":expected})
            );
        } else if provider_id == "openrouter" {
            assert_eq!(body["reasoning"]["effort"], expected);
        } else {
            assert_eq!(body["reasoning_effort"], expected);
        }
        assert!(body.get("thinking_budget").is_none());
    }
}

#[tokio::test]
async fn missing_model_configuration_rejects_model_block_requests() {
    let mut config = Config::default();
    config.llm.provider = "openai".into();
    config.llm.model = "missing-model".into();
    config.llm.openai.api_key = None;
    let provider = crate::create_llm_provider(&config).unwrap();
    let error = match provider
        .stream_model_blocks(ChatRequest::new_user("hello", "missing-model"))
        .await
    {
        Ok(_) => panic!("Unconfigured provider must reject execution"),
        Err(error) => error,
    };
    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn native_provider_defaults_preserve_identity_and_protocol() {
    let chat = r#"{"model":"model","choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let responses = r#"{"id":"resp_1","model":"model","status":"completed","output_text":"ok","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
    for (name, path, body) in [
        ("openrouter", "/v1/chat/completions", chat),
        ("openai", "/v1/responses", responses),
        ("deepseek", "/v1/chat/completions", chat),
    ] {
        let (url, server) = serve_response(
            "200 OK",
            "application/json",
            Duration::ZERO,
            vec![(Duration::ZERO, body)],
        )
        .await;
        let mut config = Config::default();
        config.llm = serde_json::from_value(serde_json::json!({
            "provider":name, "model":"model",
            (name): {"api_key":"loopback-only-key", "base_url":url, "models":[{"id":"model"}]}
        }))
        .unwrap();
        let provider = crate::create_llm_provider(&config).unwrap();
        assert_eq!(provider.name(), name);
        assert_eq!(provider.default_model(), "model");
        let response = provider
            .chat(ChatRequest::new_user("hello", "model"))
            .await
            .unwrap();
        assert_eq!(response.content, "ok");
        let (headers, _) = server.await.unwrap();
        assert!(headers.starts_with(&format!("POST {path} ")));
    }
}
