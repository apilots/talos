use super::*;
use crate::ModelBlockStreamEvent;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn read_request(socket: &mut TcpStream) {
    let mut request = Vec::new();
    loop {
        let mut bytes = [0; 4096];
        let n = socket.read(&mut bytes).await.unwrap();
        assert!(n > 0);
        request.extend_from_slice(&bytes[..n]);
        if let Some(start) = request.windows(4).position(|b| b == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&request[..start]);
            let length: usize = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            if request.len() >= start + 4 + length {
                break;
            }
        }
    }
}

async fn server(
    status: &str,
    chunks: Vec<(Duration, String)>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let status = status.to_owned();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(format!("HTTP/1.1 {status}\r\ncontent-type: text/event-stream\r\nx-request-id: test-123\r\nconnection: close\r\n\r\n").as_bytes()).await.unwrap();
        for (delay, body) in chunks {
            tokio::time::sleep(delay).await;
            if socket.write_all(body.as_bytes()).await.is_err() {
                return;
            }
        }
    });
    (url, task)
}

fn connection(url: &str, timeouts: ProviderHttpTimeouts) -> ProtocolProvider {
    let config =
        serde_json::from_value(json!({"base_url":url,"models":[{"id":"opaque"}]})).unwrap();
    ProtocolProvider::new(
        "test-connection",
        LlmProtocol::Chat,
        &config,
        "opaque",
        &timeouts,
    )
    .unwrap()
}

fn short_timeouts() -> ProviderHttpTimeouts {
    ProviderHttpTimeouts {
        connect_secs: 1,
        response_secs: 1,
        stream_idle_secs: 1,
        stream_total_secs: 3,
    }
}

#[tokio::test]
async fn progressing_stream_outlives_nonstream_total_and_preserves_tool_identity() {
    let delta = "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n";
    let (url, server) = server("200 OK", vec![
        (Duration::from_millis(400), ": heartbeat\n\n".into()),
        (Duration::from_millis(400), delta.into()),
        (Duration::from_millis(400), "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"echo\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".into()),
    ]).await;
    let provider = connection(&url, short_timeouts());
    let mut stream = provider
        .stream_model_blocks(ChatRequest::new_user("hello", "opaque"))
        .await
        .unwrap();
    let mut tool = false;
    let mut done = false;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            ModelBlockStreamEvent::BlockStarted(block) if block.tool_call.is_some() => {
                assert_eq!(block.tool_call.unwrap().call_id.as_deref(), Some("call-1"));
                tool = true;
            }
            ModelBlockStreamEvent::Done { .. } => done = true,
            _ => {}
        }
    }
    assert!(tool && done);
    server.await.unwrap();
}

#[tokio::test]
async fn idle_stream_timeout_is_typed_in_both_stream_apis() {
    for (protocol, blocks) in [
        (LlmProtocol::Chat, false),
        (LlmProtocol::Chat, true),
        (LlmProtocol::Messages, false),
        (LlmProtocol::Messages, true),
        (LlmProtocol::Responses, false),
        (LlmProtocol::Responses, true),
    ] {
        let (url, server) = server(
            "200 OK",
            vec![(Duration::from_secs(2), ": late\n\n".into())],
        )
        .await;
        let mut provider = connection(&url, short_timeouts());
        provider.protocol = protocol;
        let error = if blocks {
            provider
                .stream_model_blocks(ChatRequest::new_user("hello", "opaque"))
                .await
                .unwrap()
                .next()
                .await
                .unwrap()
                .unwrap_err()
        } else {
            provider
                .stream(ChatRequest::new_user("hello", "opaque"))
                .await
                .unwrap()
                .next()
                .await
                .unwrap()
                .unwrap_err()
        };
        assert_eq!(error.error_code(), ErrorCode::Timeout);
        assert!(error.to_api_error().message.contains("timed out"));
        server.abort();
    }
}

#[tokio::test]
async fn heartbeat_activity_does_not_extend_stream_total() {
    let (url, server) = server(
        "200 OK",
        vec![(Duration::from_millis(200), ": heartbeat\n\n".into()); 20],
    )
    .await;
    let mut timeouts = short_timeouts();
    timeouts.stream_total_secs = 1;
    let provider = connection(&url, timeouts);
    let mut stream = provider
        .stream_model_blocks(ChatRequest::new_user("hello", "opaque"))
        .await
        .unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap_err().error_code(),
        ErrorCode::Timeout
    );
    server.abort();
}

#[tokio::test]
async fn response_body_timeout_remains_typed_and_malformed_sse_is_distinct() {
    let (url, server) = server("200 OK", vec![(Duration::from_secs(2), "{}".into())]).await;
    assert_eq!(
        connection(&url, short_timeouts())
            .chat(ChatRequest::new_user("hello", "opaque"))
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Timeout
    );
    server.abort();
    let (url, task) = self::server(
        "200 OK",
        vec![(Duration::ZERO, "data: not-json\n\n".into())],
    )
    .await;
    let mut stream = connection(&url, short_timeouts())
        .stream(ChatRequest::new_user("hello", "opaque"))
        .await
        .unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap_err().error_code(),
        ErrorCode::InternalError
    );
    task.await.unwrap();
}

#[tokio::test]
async fn http_rejections_classify_codes_without_retaining_remote_messages() {
    for (body, code) in [
        (json!({"error":{"code":"context_length_exceeded","message":"secret-body","param":"messages"}}).to_string(), ErrorCode::ContextWindowExceeded),
        (json!({"error":{"type":"invalid_request_error","param":"reasoning_effort","message":"secret-body"}}).to_string(), ErrorCode::InternalError),
        ("secret-body".repeat(1000), ErrorCode::InternalError),
    ] {
        let (url, task) = server("400 Bad Request", vec![(Duration::ZERO, body)]).await;
        let error = connection(&url, short_timeouts()).chat(ChatRequest::new_user("hello", "opaque")).await.unwrap_err();
        assert_eq!(error.error_code(), code);
        assert!(error.to_api_error().message.contains("HTTP 400"));
        assert!(!error.to_api_error().message.contains("stream"));
        let diagnostic = format!("{error:?}");
        assert!(!diagnostic.contains("secret-body"));
        assert!(!diagnostic.contains(&url));
        task.await.unwrap();
    }
}

#[tokio::test]
async fn dropping_the_consumer_closes_an_idle_upstream_without_waiting_for_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut byte = [0; 1];
        assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
    });
    let provider = connection(&url, ProviderHttpTimeouts::default());
    let stream = provider
        .stream_model_blocks(ChatRequest::new_user("hello", "opaque"))
        .await
        .unwrap();
    drop(stream);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn waiting_for_response_headers_is_bounded_and_classified() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    });
    let error = connection(&url, short_timeouts())
        .stream(ChatRequest::new_user("hello", "opaque"))
        .await
        .err()
        .unwrap();
    assert_eq!(error.error_code(), ErrorCode::Timeout);
    task.abort();
}
