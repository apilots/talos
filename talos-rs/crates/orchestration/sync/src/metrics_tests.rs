use super::*;
use crate::metrics_wire::{MetadataCounts, PeerMetrics};
use pretty_assertions::assert_eq;

fn account() -> AccountMetrics {
    AccountMetrics {
        api_version: 1,
        observed_at: 1_700_000_123_456,
        last_flushed_at: Some(1_700_000_120_123),
        complete: true,
        account_id: "account-a".into(),
        metadata: MetadataCounts {
            computers: 2,
            projects: 3,
            sessions: 4,
            files: 5,
        },
        peers: vec![PeerMetrics {
            peer_id: "peer-a".into(),
            computer_ids: vec!["computer-a".into()],
            connections: 2,
            document_rooms: 3,
            last_activity_at: Some(1_700_000_122_999),
            inbound_bytes_per_second: 10.5,
            outbound_bytes_per_second: 20.5,
            updates_per_second: 1.5,
            commit_p95_ms: Some(4.5),
        }],
    }
}

#[test]
fn projection_preserves_observations_and_converts_only_timestamps_to_seconds() {
    let result = project(account(), "account-a", "https://sync.example".into()).unwrap();
    assert_eq!(
        result,
        SyncMetricsResponse {
            endpoint: "https://sync.example".into(),
            control_plane_peer_id: None,
            observed_at: 1_700_000_123,
            last_flushed_at: Some(1_700_000_120),
            complete: true,
            metadata: SyncMetadataCounts {
                computers: 2,
                projects: 3,
                sessions: 4,
                files: 5
            },
            peers: vec![SyncPeerMetrics {
                peer_id: "peer-a".into(),
                computer_ids: vec!["computer-a".into()],
                connections: 2,
                document_rooms: 3,
                last_activity_at: Some(1_700_000_122),
                inbound_bytes_per_second: 10.5,
                outbound_bytes_per_second: 20.5,
                updates_per_second: 1.5,
                commit_p95_ms: Some(4.5),
            }],
        }
    );
}

#[test]
fn cross_account_and_unsupported_version_fail_before_exposing_observations() {
    assert_eq!(
        project(account(), "other-account", "https://sync.example".into())
            .unwrap_err()
            .error_code(),
        ErrorCode::PermissionDenied
    );
    let mut future = account();
    future.api_version = 2;
    assert_eq!(
        project(future, "account-a", "https://sync.example".into())
            .unwrap_err()
            .error_code(),
        ErrorCode::ServiceUnavailable
    );
}

#[test]
fn endpoint_conversion_keeps_origin_and_rejects_insecure_or_credential_selectors() {
    assert_eq!(
        metrics_url("wss://sync.example:9220/v1/sync")
            .unwrap()
            .as_str(),
        "https://sync.example:9220/v1/metrics/account"
    );
    for endpoint in [
        "ws://sync.example/v1/sync",
        "wss://secret@sync.example/v1/sync",
        "wss://sync.example/v1/sync?account=other",
        "wss://sync.example/v1/sync#fragment",
    ] {
        assert_eq!(
            metrics_url(endpoint).unwrap_err().error_code(),
            ErrorCode::InvalidRequest
        );
    }
}

struct HttpsFixture {
    endpoint: String,
    certificate: Vec<u8>,
    requests: tokio::sync::mpsc::UnboundedReceiver<String>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for HttpsFixture {
    fn drop(&mut self) {
        // The owned test listener has no production drain obligations.
        self.task.abort();
    }
}

async fn https_fixture(status: &str, body: Vec<u8>) -> HttpsFixture {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let tls = tokio_rustls::rustls::ServerConfig::builder_with_provider(Arc::new(
        tokio_rustls::rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        tokio_rustls::rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der())
            .into(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "wss://localhost:{}/v1/sync",
        listener.local_addr().unwrap().port()
    );
    let (sent, requests) = tokio::sync::mpsc::unbounded_channel();
    let status = status.to_owned();
    let task = tokio::spawn(async move {
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let mut calls = 0;
        while let Ok((tcp, _)) = listener.accept().await {
            let Ok(mut socket) = acceptor.accept(tcp).await else {
                continue;
            };
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 {
                if socket.read_exact(&mut byte).await.is_err() {
                    break;
                }
                request.push(byte[0]);
            }
            if sent.send(String::from_utf8(request).unwrap()).is_err() {
                break;
            }
            // A followed redirect would succeed on the second request, making
            // the redirect regression fail without timing-based assertions.
            let (status, response) = if calls == 0 {
                (status.as_str(), body.clone())
            } else {
                ("200 OK", serde_json::to_vec(&account()).unwrap())
            };
            calls += 1;
            let headers = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n", response.len());
            if socket.write_all(headers.as_bytes()).await.is_err() {
                continue;
            }
            // Oversized-response coverage intentionally closes its reader early.
            if socket.write_all(&response).await.is_err() {
                continue;
            }
            if socket.shutdown().await.is_err() {
                continue;
            }
        }
    });
    HttpsFixture {
        endpoint,
        certificate: cert.pem().into_bytes(),
        requests,
        task,
    }
}

fn credential(account: &str) -> VerifiedCredential {
    VerifiedCredential {
        account_id: account.into(),
        access_token: "fixture-token".into(),
    }
}

#[tokio::test]
async fn https_sends_only_verified_bearer_to_account_path_and_requires_trusted_ca() {
    let mut server = https_fixture("200 OK", serde_json::to_vec(&account()).unwrap()).await;
    let response = read_account_metrics(
        &server.endpoint,
        Some(&server.certificate),
        credential("account-a"),
    )
    .await
    .unwrap();
    assert_eq!(response.peers.len(), 1);
    let request = server.requests.recv().await.unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /v1/metrics/account http/1.1\r\n"));
    assert!(request.contains("authorization: bearer fixture-token\r\n"));
    assert_eq!(
        read_account_metrics(&server.endpoint, None, credential("account-a"))
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::ServiceUnavailable
    );
}

#[tokio::test]
async fn https_preserves_auth_errors_and_rejects_redirects_and_oversized_bodies() {
    let rejection =
        talos_sync_types::SyncError::new(talos_sync_types::SyncErrorCode::Unauthenticated);
    let server = https_fixture("401 Unauthorized", serde_json::to_vec(&rejection).unwrap()).await;
    assert_eq!(
        read_account_metrics(
            &server.endpoint,
            Some(&server.certificate),
            credential("account-a")
        )
        .await
        .unwrap_err()
        .error_code(),
        ErrorCode::Unauthenticated
    );
    let server = https_fixture(
        "302 Found\r\nLocation: /v1/metrics/account?other=account",
        vec![],
    )
    .await;
    assert_eq!(
        read_account_metrics(
            &server.endpoint,
            Some(&server.certificate),
            credential("account-a")
        )
        .await
        .unwrap_err()
        .error_code(),
        ErrorCode::ServiceUnavailable
    );
    let server = https_fixture("200 OK", vec![b' '; MAX_RESPONSE_BYTES + 1]).await;
    assert_eq!(
        read_account_metrics(
            &server.endpoint,
            Some(&server.certificate),
            credential("account-a")
        )
        .await
        .unwrap_err()
        .error_code(),
        ErrorCode::InternalError
    );
}
