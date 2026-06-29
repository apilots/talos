use super::*;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_json_server(body: &'static str) -> String {
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
        let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("response should write");
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn execute_payload_reads_web_search_provider_config_from_context() {
    let base_url = spawn_json_server(
        r#"{
                "Results": [
                    {
                        "FirstURL": "https://www.rust-lang.org/tools",
                        "Text": "Rust Tools - Official tools page"
                    }
                ]
            }"#,
    )
    .await;
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.tools.web_search.duckduckgo.base_url = Some(base_url);
    let context = ToolContext::new_with_kernel_config(
        "web-search-session",
        "user-1",
        Some(Arc::new(kernel_config)),
    );

    let payload = WebSearchTool
        .execute_payload(
            serde_json::json!({
                "query": "rust tools",
                "allowed_domains": ["rust-lang.org"],
                "limit": 1
            }),
            context,
        )
        .await
        .expect("web search should execute");

    let ToolOutputPayload::Function {
        success,
        structured_content,
        content_items,
        ..
    } = payload
    else {
        panic!("web search should return a function payload");
    };
    assert!(success);
    assert_eq!(
        structured_content
            .as_ref()
            .and_then(|value| value.get("provider"))
            .and_then(Value::as_str),
        Some("duckduckgo")
    );
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::WebReference { url, .. })
            if url == "https://www.rust-lang.org/tools"
    ));
}
