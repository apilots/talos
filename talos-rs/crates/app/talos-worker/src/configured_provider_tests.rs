use talos_config::Config;
use talos_llm_provider::{ChatRequest, ReasoningEffort};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn custom_protocol_sends_session_thinking_controls_without_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..6 {
            let (mut socket, _) = listener.accept().await.unwrap();
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
