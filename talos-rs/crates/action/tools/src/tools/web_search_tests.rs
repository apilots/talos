use super::*;
use std::sync::Arc;
use talos_config::{AgentKernelConfig, RuntimeEnv, WebSearchProviderConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn spawn_json_server(body: impl Into<String>) -> String {
    let body = body.into();
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
    let source = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<reqwest::Error>()
        .expect("Firecrawl timeout must come from its HTTP client");
    assert!(source.is_timeout());
    task.abort();
    if let Err(error) = task.await {
        assert!(error.is_cancelled());
    }
}

fn search_context(provider: WebSearchProviderConfig, url: String) -> ToolContext {
    let mut config = AgentKernelConfig::default();
    config.tools.web_search.provider = provider;
    let runtime = match provider {
        WebSearchProviderConfig::Firecrawl => &mut config.tools.web_search.firecrawl,
        WebSearchProviderConfig::Brave => &mut config.tools.web_search.brave,
        WebSearchProviderConfig::Tavily => &mut config.tools.web_search.tavily,
    };
    runtime.base_url = Some(url);
    runtime.api_key = Some("test-key".into());
    ToolContext::new_with_kernel_config("search", "user-1", Some(Arc::new(config)))
}

#[tokio::test]
async fn execute_payload_rejects_oversized_search_bodies_without_eof() {
    for provider in [
        WebSearchProviderConfig::Firecrawl,
        WebSearchProviderConfig::Brave,
        WebSearchProviderConfig::Tavily,
    ] {
        let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n100001\r\n".to_vec();
        response.resize(response.len() + 1024 * 1024 + 1, b' ');
        response.extend_from_slice(b"\r\n");
        let (url, task) = spawn_incomplete_response(response).await;
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            WebSearchTool.execute_payload(
                serde_json::json!({"query": "rust tools", "limit": 1}),
                search_context(provider, url),
            ),
        )
        .await
        .expect("search must reject excess bytes without waiting for EOF")
        .expect_err("oversized search response must fail");
        assert_eq!(
            error.error_code(),
            talos_error::ErrorCode::ToolExecutionFailed
        );
        assert!(
            std::error::Error::source(&error).is_none(),
            "must reject before JSON parsing or timeout"
        );
        task.abort();
        if let Err(error) = task.await {
            assert!(error.is_cancelled());
        }
    }
}

#[tokio::test]
async fn execute_payload_accepts_search_bodies_at_the_limit() {
    for (provider, body) in [
        (
            WebSearchProviderConfig::Firecrawl,
            r#"{"success":true,"data":{"web":[{"url":"https://example.com","title":"Example","description":"Result"}]}}"#,
        ),
        (
            WebSearchProviderConfig::Brave,
            r#"{"web":{"results":[{"url":"https://example.com","title":"Example","description":"Result"}]}}"#,
        ),
        (
            WebSearchProviderConfig::Tavily,
            r#"{"query":"rust tools","response_time":0.01,"results":[{"url":"https://example.com","title":"Example","content":"Result","score":1.0}]}"#,
        ),
    ] {
        let mut body = body.to_string();
        body.extend(std::iter::repeat_n(' ', 1024 * 1024 - body.len()));
        let url = spawn_json_server(body).await;
        let payload = WebSearchTool
            .execute_payload(
                serde_json::json!({"query": "rust tools", "limit": 1}),
                search_context(provider, url),
            )
            .await
            .unwrap();
        let ToolOutputPayload::Function {
            success,
            structured_content,
            ..
        } = payload
        else {
            panic!("search must return a function payload");
        };
        assert!(success);
        assert_eq!(
            structured_content.unwrap()["hits"],
            serde_json::json!([{
                "title":"Example", "url":"https://example.com", "snippet":"Result", "source_domain":"example.com", "rank":1
            }])
        );
    }
}

#[tokio::test]
async fn execute_payload_rejects_http_errors_without_reading_the_body() {
    let (url, task) = spawn_incomplete_response(
        b"HTTP/1.1 500 Internal Server Error\r\nTransfer-Encoding: chunked\r\n\r\n12\r\nprivate-api-secret\r\n".to_vec(),
    ).await;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        WebSearchTool.execute_payload(
            serde_json::json!({"query":"rust tools"}),
            search_context(WebSearchProviderConfig::Firecrawl, url),
        ),
    )
    .await
    .expect("HTTP errors must not wait for an unbounded body")
    .unwrap_err();
    let source = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<reqwest::Error>()
        .unwrap();
    assert_eq!(
        source.status(),
        Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert!(source.url().is_none());
    assert_eq!(error.to_api_error().message, "Web search request failed.");
    task.abort();
    if let Err(error) = task.await {
        assert!(error.is_cancelled());
    }
}

async fn spawn_rate_limited_server() -> (String, tokio::task::JoinHandle<Vec<std::time::Instant>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut attempts = Vec::new();
        for _ in 0..4 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            attempts.push(std::time::Instant::now());
            socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
        }
        attempts
    });
    (url, task)
}

#[tokio::test]
async fn execute_payload_retries_tavily_three_times_at_a_fixed_interval() {
    let (url, server) = spawn_rate_limited_server().await;
    let mut config = AgentKernelConfig::default();
    config.tools.web_search.provider = WebSearchProviderConfig::Tavily;
    config.tools.web_search.tavily.base_url = Some(url);
    config.tools.web_search.tavily.api_key = Some("test-key".into());
    config.tools.web_search.tavily.timeout_ms = 4_500;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        WebSearchTool.execute_payload(
            serde_json::json!({"query": "rust tools"}),
            ToolContext::new_with_kernel_config("search", "user-1", Some(Arc::new(config))),
        ),
    )
    .await
    .unwrap()
    .expect_err("persistent rate limiting must fail after three retries");
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ToolExecutionFailed
    );
    assert_eq!(
        std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<reqwest::Error>()
            .unwrap()
            .status(),
        Some(reqwest::StatusCode::TOO_MANY_REQUESTS)
    );
    let attempts = tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attempts.len(), 4);
    assert!(attempts
        .windows(2)
        .all(|pair| pair[1].duration_since(pair[0]) >= std::time::Duration::from_secs(1)));
}

#[tokio::test]
async fn execute_payload_bounds_tavily_retry_wait_by_the_configured_deadline() {
    let (url, server) = spawn_rate_limited_server().await;
    let mut config = AgentKernelConfig::default();
    config.tools.web_search.provider = WebSearchProviderConfig::Tavily;
    config.tools.web_search.tavily.base_url = Some(url);
    config.tools.web_search.tavily.api_key = Some("test-key".into());
    config.tools.web_search.tavily.timeout_ms = 50;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        WebSearchTool.execute_payload(
            serde_json::json!({"query": "rust tools"}),
            ToolContext::new_with_kernel_config("search", "user-1", Some(Arc::new(config))),
        ),
    )
    .await
    .unwrap()
    .expect_err("the deadline must cancel the retry wait");
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ToolExecutionFailed
    );
    assert!(std::error::Error::source(&error)
        .unwrap()
        .is::<tokio::time::error::Elapsed>());
    server.abort();
    if let Err(error) = server.await {
        assert!(error.is_cancelled());
    }
}
