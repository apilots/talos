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
        provider
            .chat(ChatRequest::new_user("Off", "model"))
            .await
            .unwrap();
        let mut enabled = ChatRequest::new_user("On", "model");
        enabled.reasoning_effort = Some(ReasoningEffort::Medium);
        provider.chat(enabled).await.unwrap();
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
    header_delay: Duration,
    chunks: Vec<(Duration, &'static str)>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let status = status.to_owned();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (headers, _) = read_request(&mut socket).await;
        assert!(headers.starts_with("POST "));
        tokio::time::sleep(header_delay).await;
        let headers = format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n"
        );
        if socket.write_all(headers.as_bytes()).await.is_err() {
            return;
        }
        for (delay, chunk) in chunks {
            tokio::time::sleep(delay).await;
            if socket.write_all(chunk.as_bytes()).await.is_err() {
                return;
            }
        }
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
            let (url, task) =
                serve_response("200 OK", header_delay, vec![(Duration::from_secs(2), "{}")]).await;
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
        let (url, task) = serve_response("400 Bad Request", Duration::ZERO, vec![
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
