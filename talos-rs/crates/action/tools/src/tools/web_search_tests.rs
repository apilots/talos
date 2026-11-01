use super::*;
use std::sync::Arc;
use talos_config::{AgentKernelConfig, RuntimeEnv, WebSearchProviderConfig};
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

async fn spawn_incomplete_response(response: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        if let Err(error) = socket.write_all(&response).await {
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ));
            return;
        }
        // Withhold EOF so the adapter must enforce its own deadline or byte limit.
        std::future::pending::<()>().await;
    });
    (url, task)
}

#[test]
fn parameters_schema_exposes_the_provider_neutral_contract() {
    let schema = WebSearchTool
        .parameters_schema()
        .expect("web search should define a schema");
    assert_eq!(
        schema,
        serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query text."
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain allowlist."
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain blocklist."
                },
                "recency_days": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Requested freshness window in days. The structured response reports whether the selected provider applied it."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20,
                    "description": "Maximum number of hits to return."
                }
            }
        })
    );
}

#[tokio::test]
async fn execute_payload_reads_web_search_provider_config_from_context() {
    let base_url = spawn_json_server(
        r#"{
                "success": true,
                "data": {
                    "web": [{
                        "url": "https://www.rust-lang.org/tools",
                        "title": "Rust Tools",
                        "description": "Official tools page"
                    }]
                }
            }"#,
    )
    .await;
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.tools.web_search.firecrawl.base_url = Some(base_url);
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
        structured_content,
        Some(serde_json::json!({
            "query": "rust tools",
            "provider": "firecrawl",
            "recency_days": null,
            "freshness_applied": false,
            "hits": [{
                "title": "Rust Tools",
                "url": "https://www.rust-lang.org/tools",
                "snippet": "Official tools page",
                "source_domain": "rust-lang.org",
                "rank": 1
            }]
        }))
    );
    assert_eq!(
        content_items,
        vec![TalosContentItem::WebReference {
            url: "https://www.rust-lang.org/tools".to_string(),
            title: Some("Rust Tools".to_string()),
            citation_id: Some("web-1".to_string()),
            snippet: Some("Official tools page".to_string()),
            source_domain: Some("rust-lang.org".to_string()),
            published_at: None,
            provider_rank: Some(1),
        }]
    );
}

#[tokio::test]
async fn execute_payload_reads_api_key_from_context_runtime_env() {
    let base_url = spawn_json_server(
        r#"{
                "web": {
                    "results": [{
                        "url": "https://www.rust-lang.org/tools",
                        "title": "Rust Tools",
                        "description": "Official tools page"
                    }]
                }
            }"#,
    )
    .await;
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.tools.web_search.provider = WebSearchProviderConfig::Brave;
    kernel_config.tools.web_search.brave.base_url = Some(base_url);
    kernel_config.tools.web_search.brave.api_key =
        Some("${TALOS_TEST_WEB_SEARCH_BRAVE_API_KEY}".to_string());
    kernel_config.runtime_env = RuntimeEnv::load_from_iter(
        std::path::Path::new("__talos_missing_env_file__"),
        [("TALOS_TEST_WEB_SEARCH_BRAVE_API_KEY", "brave-key")],
    )
    .expect("runtime env should load");
    let context = ToolContext::new_with_kernel_config(
        "web-search-session",
        "user-1",
        Some(Arc::new(kernel_config)),
    );

    let payload = WebSearchTool
        .execute_payload(
            serde_json::json!({
                "query": "rust tools",
                "limit": 1
            }),
            context,
        )
        .await
        .expect("web search should execute with runtime env key");

    let ToolOutputPayload::Function {
        success,
        structured_content,
        ..
    } = payload
    else {
        panic!("web search should return a function payload");
    };
    assert!(success);
    assert_eq!(
        structured_content
            .expect("structured content should exist")
            .get("provider"),
        Some(&serde_json::json!("brave"))
    );
}

#[tokio::test]
async fn execute_payload_enforces_firecrawl_configured_deadline() {
    let (url, task) = spawn_incomplete_response(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{"
            .to_vec(),
    )
    .await;
    let mut config = AgentKernelConfig::default();
    config.tools.web_search.firecrawl.base_url = Some(url);
    config.tools.web_search.firecrawl.timeout_ms = 50;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        WebSearchTool.execute_payload(
            serde_json::json!({"query": "rust tools"}),
            ToolContext::new_with_kernel_config("search", "user-1", Some(Arc::new(config))),
        ),
    )
    .await
    .expect("Firecrawl must enforce the configured deadline without EOF")
    .expect_err("incomplete Firecrawl body must time out");
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ToolExecutionFailed
    );
    assert!(std::error::Error::source(&error)
        .unwrap()
        .is::<tokio::time::error::Elapsed>());
    task.abort();
    if let Err(error) = task.await {
        assert!(error.is_cancelled());
    }
}

#[tokio::test]
async fn execute_payload_rejects_oversized_brave_body_without_eof() {
    let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n100001\r\n".to_vec();
    response.resize(response.len() + 1024 * 1024 + 1, b' ');
    response.extend_from_slice(b"\r\n");
    let (url, task) = spawn_incomplete_response(response).await;
    let mut config = AgentKernelConfig::default();
    config.tools.web_search.provider = WebSearchProviderConfig::Brave;
    config.tools.web_search.brave.base_url = Some(url);
    config.tools.web_search.brave.api_key = Some("brave-key".to_string());
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        WebSearchTool.execute_payload(
            serde_json::json!({"query": "rust tools", "limit": 1}),
            ToolContext::new_with_kernel_config("search", "user-1", Some(Arc::new(config))),
        ),
    )
    .await
    .expect("Brave must reject excess bytes without waiting for EOF")
    .expect_err("oversized search response must fail");
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ToolExecutionFailed
    );
    task.abort();
    if let Err(error) = task.await {
        assert!(error.is_cancelled());
    }
}
