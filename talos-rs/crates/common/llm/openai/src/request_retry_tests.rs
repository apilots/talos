use super::*;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

type Requests = Arc<Mutex<Vec<Vec<u8>>>>;

async fn gateway(statuses: Vec<u16>, body: &'static str) -> (String, Requests, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let server = tokio::spawn(async move {
        for status in statuses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = socket.read(&mut buffer).await.unwrap();
                assert_ne!(count, 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..offset]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")?
                                .parse()
                                .ok()
                        })
                        .unwrap();
                    if bytes.len() >= offset + 4 + length {
                        break;
                    }
                }
            }
            observed.lock().unwrap().push(bytes);
            let payload = if status == 200 { body } else { "{}" };
            let reply = format!(
                "HTTP/1.1 {status} Test\r\ncontent-length: {}\r\nx-request-id: req-{status}\r\nconnection: close\r\n\r\n{payload}",
                payload.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (format!("http://{address}"), requests, server)
}

const CHAT_STREAM: &str =
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"recovered\"}}]}\n\ndata: [DONE]\n\n";
const RESPONSES_STREAM: &str = "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"recovered\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n";

#[tokio::test]
async fn gateway_failures_recover_with_identical_stream_requests() {
    for protocol in [OpenAIProtocol::Chat, OpenAIProtocol::Responses] {
        let body = if protocol == OpenAIProtocol::Chat {
            CHAT_STREAM
        } else {
            RESPONSES_STREAM
        };
        let (url, requests, server) = gateway(vec![502, 503, 200], body).await;
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-key")
                .with_base_url(url)
                .with_protocol(protocol),
        );
        let events = collect_model_events(
            provider
                .stream_model_blocks(ChatRequest::new_user("task", "test"))
                .await
                .unwrap(),
        )
        .await;
        assert!(events.iter().any(|event| matches!(event,
            ModelBlockStreamEvent::BlockDelta { delta: ModelBlockDelta::AssistantText { delta }, .. } if delta == "recovered")));
        let observed = requests.lock().unwrap();
        assert_eq!(observed.len(), 3);
        assert!(observed.windows(2).all(|pair| pair[0] == pair[1]));
        server.abort();
    }
}

#[tokio::test]
async fn gateway_timeout_recovery_applies_to_non_streaming_requests() {
    for (protocol, body) in [
        (OpenAIProtocol::Chat, "{\"model\":\"test\",\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"recovered\"},\"finish_reason\":\"stop\"}]}"),
        (OpenAIProtocol::Responses, "{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"recovered\"}]}]}")
    ] {
        let (url, requests, server) = gateway(vec![504, 200], body).await;
        let provider = OpenAIProvider::new(OpenAIConfig::new("test-key").with_base_url(url).with_protocol(protocol));
        let response = provider.chat(ChatRequest::new_user("task", "test")).await.unwrap();
        assert_eq!(response.content, "recovered");
        assert_eq!(requests.lock().unwrap().len(), 2);
        server.abort();
    }
}

#[tokio::test]
async fn exhausted_gateway_retries_keep_the_final_typed_error() {
    let (url, requests, server) =
        gateway(vec![502, 503, 502, 503, 502, 504, 200], CHAT_STREAM).await;
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-key").with_base_url(url));
    let error = provider
        .stream_model_blocks(ChatRequest::new_user("task", "test"))
        .await
        .err()
        .unwrap();
    assert_eq!(error.error_code(), talos_error::ErrorCode::Timeout);
    assert!(error
        .diagnostic_message()
        .contains("upstream_request_id=req-504"));
    assert_eq!(requests.lock().unwrap().len(), 6);
    server.abort();
}

#[tokio::test]
async fn permanent_or_rate_limit_rejections_are_returned_without_gateway_retry() {
    for (status, expected) in [
        (400, talos_error::ErrorCode::InternalError),
        (401, talos_error::ErrorCode::ProviderUnavailable),
        (429, talos_error::ErrorCode::UsageLimitExceeded),
    ] {
        let (url, requests, server) = gateway(vec![status, 200], CHAT_STREAM).await;
        let provider = OpenAIProvider::new(OpenAIConfig::new("test-key").with_base_url(url));
        let error = provider
            .stream_model_blocks(ChatRequest::new_user("task", "test"))
            .await
            .err()
            .unwrap();
        assert_eq!(error.error_code(), expected);
        assert_eq!(requests.lock().unwrap().len(), 1);
        server.abort();
    }
}

#[tokio::test]
async fn accepted_stream_failure_is_not_replayed() {
    let partial = "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call\",\"function\":{\"name\":\"shell\",\"arguments\":\"{}\"}}]}}]}\n\ndata: {\"error\":{\"type\":\"server_overloaded\"}}\n\n";
    let (url, requests, server) = gateway(vec![200, 200], partial).await;
    let provider = OpenAIProvider::new(OpenAIConfig::new("test-key").with_base_url(url));
    let error = collect_first_model_error(
        provider
            .stream_model_blocks(ChatRequest::new_user("task", "test"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(error.error_code(), talos_error::ErrorCode::InternalError);
    assert_eq!(requests.lock().unwrap().len(), 1);
    server.abort();
}
