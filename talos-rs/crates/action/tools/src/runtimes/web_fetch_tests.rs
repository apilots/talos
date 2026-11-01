use super::*;

#[tokio::test]
async fn web_fetch_rejects_zero_max_chars() {
    let error = run(&WebFetchRequest {
        url: "https://example.com".to_string(),
        prompt: None,
        max_chars: 0,
    })
    .await
    .expect_err("zero max chars should fail");
    assert!(error
        .to_string()
        .contains("max_chars must be greater than zero"));
}

#[test]
fn web_fetch_warns_for_private_urls() {
    let url = reqwest::Url::parse("http://127.0.0.1:3000").expect("url should parse");
    assert!(private_url_warning(&url).is_some());
}

struct HttpFixture {
    url: String,
    finish: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl HttpFixture {
    async fn start(response: Vec<u8>) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (finish, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            if let Err(error) = socket.write_all(&response).await {
                // A bounded reader may close before the rest of this response is sent.
                assert!(matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                ));
                return;
            }
            // Keep EOF under test control, including incomplete/chunked bodies.
            let _ = stopped.await;
        });
        Self { url, finish, task }
    }

    async fn finish(self) {
        // The writer may already have observed the reader closing at its limit.
        let _ = self.finish.send(());
        self.task.await.unwrap();
    }
}

#[tokio::test]
async fn web_fetch_bounds_chunked_body_without_waiting_for_eof() {
    let mut body = "你好".as_bytes().to_vec();
    body.resize(2 * MAX_RESPONSE_BYTES, b'x');
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
        body.len(),
    ).into_bytes();
    response.extend_from_slice(&body);
    response.extend_from_slice(b"\r\n"); // Deliberately omit the final zero-length chunk.
    let fixture = HttpFixture::start(response).await;
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        run(&WebFetchRequest {
            url: fixture.url.clone(),
            prompt: None,
            max_chars: usize::MAX,
        }),
    )
    .await
    .expect("WebFetch must stop at its byte budget without EOF")
    .unwrap();
    assert!(response.truncated);
    assert!(response.content.starts_with("你好"));
    assert_eq!(
        response.content.len(),
        MAX_RESPONSE_BYTES + TRUNCATION_MARKER.len()
    );
    assert!(response.content.ends_with(TRUNCATION_MARKER));
    fixture.finish().await;
}

#[tokio::test]
async fn web_fetch_tool_preserves_charset_bom_and_character_truncation() {
    use crate::{Tool, ToolContext, WebFetchTool};
    let cases = [
        (
            "text/html; charset=windows-1252",
            b"<title>Caf\xe9</title><p>R\xe9sum\xe9</p>".to_vec(),
            5,
            "Café ...[truncated]",
            Some("Café"),
            true,
        ),
        (
            "text/plain; charset=windows-1252",
            [b"\xef\xbb\xbf".as_slice(), "你好".as_bytes()].concat(),
            100,
            "你好",
            None,
            false,
        ),
    ];
    for (mime, body, max_chars, content, title, truncated) in cases {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\r\n",
            body.len(),
        )
        .into_bytes();
        response.extend_from_slice(&body);
        let fixture = HttpFixture::start(response).await;
        let result = WebFetchTool
            .execute_payload(
                serde_json::json!({"url":fixture.url,"max_chars":max_chars}),
                ToolContext::new("web-fetch-test", "user-1"),
            )
            .await
            .unwrap();
        let result = result.structured_content().unwrap();
        assert_eq!(result["content"], content);
        assert_eq!(result["title"], serde_json::json!(title));
        assert_eq!(result["truncated"], truncated);
        fixture.finish().await;
    }
}

#[tokio::test]
async fn web_fetch_times_out_a_stalled_body_with_its_typed_source() {
    use std::error::Error;
    let fixture = HttpFixture::start(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 1\r\n\r\n".to_vec(),
    )
    .await;
    let error = tokio::time::timeout(
        REQUEST_TIMEOUT + Duration::from_secs(3),
        run(&WebFetchRequest {
            url: format!("{}?token=private", fixture.url),
            prompt: None,
            max_chars: 100,
        }),
    )
    .await
    .expect("the request deadline must include body reads")
    .unwrap_err();
    assert!(error.is_code(ErrorCode::ToolExecutionFailed));
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<reqwest::Error>()
        .unwrap();
    assert!(source.is_timeout());
    assert!(source.url().is_none());
    fixture.finish().await;
}
