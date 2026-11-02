use super::{fixture, fixture_with_options, runner_wss::tls_listener, runner_wss::EmptyAdapter};
use crate::*;
use futures_util::{SinkExt, StreamExt};
use pretty_assertions::assert_eq;
use std::{sync::Arc, time::Duration};
use talos_sync_types::{SyncControlFrame, SyncControlMessage, SyncErrorCode, SyncPage};
use tokio::{sync::mpsc, time::Instant};
use tokio_tungstenite::tungstenite::Message;

async fn wait_connection(handle: &SyncHandle, expected: SyncConnectionState) {
    let mut status = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(2), async {
        while status.borrow().connection != expected {
            status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn rejected_credentials_use_the_configured_fixed_reconnect_interval() {
    let fixture = fixture().await;
    let (attempts, mut received) = mpsc::unbounded_channel();
    let (errors, code) = tokio::sync::watch::channel(SyncErrorCode::Unauthenticated);
    let handle = Arc::new(fixture.sync)
        .start(
            Arc::new(EmptyAdapter),
            SyncStartParams {
                binding: fixture.binding,
                credentials: Arc::new(move || {
                    let code = *code.borrow();
                    attempts.send(code).unwrap();
                    Box::pin(async move { Err(talos_sync_types::SyncError::new(code)) })
                }),
                trusted_ca_pem: None,
                reconnect_delay: Duration::from_secs(1),
            },
        )
        .unwrap();
    assert_eq!(received.recv().await, Some(SyncErrorCode::Unauthenticated));
    wait_connection(&handle, SyncConnectionState::Disconnected).await;
    errors.send_replace(SyncErrorCode::ServiceUnavailable);
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(500)).await;
    tokio::time::resume();
    // Let local SQLite I/O finish if an incorrect early retry started.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(received.try_recv().is_err());
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(500)).await;
    tokio::time::resume();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), received.recv())
            .await
            .unwrap(),
        Some(SyncErrorCode::ServiceUnavailable)
    );
    wait_connection(&handle, SyncConnectionState::Disconnected).await;
    // No SDK-internal retry may run inside one durable attempt.
    assert!(received.try_recv().is_err());
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn fixed_reconnect_interval_recovers_short_feeds_and_refreshed_credentials() {
    let (listener, acceptor, trusted_ca) = tls_listener().await;
    let endpoint = format!(
        "wss://localhost:{}/v1/sync",
        listener.local_addr().unwrap().port()
    );
    let (disconnect, mut dropped) = mpsc::channel(1);
    let peer = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let tls = acceptor.accept(tcp).await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tls).await.unwrap();
            loop {
                let message = tokio::select! {
                    _ = dropped.recv() => break,
                    message = socket.next() => message,
                };
                let Some(Ok(message)) = message else { return };
                let response = match message {
                    Message::Text(text) if text == "ping" => Message::Text("pong".into()),
                    Message::Text(text) => {
                        let request: SyncControlFrame = serde_json::from_str(&text).unwrap();
                        let body = match request.body {
                            SyncControlMessage::Hello { .. } => SyncControlMessage::HelloAccepted {
                                hub_id: "hub-1".into(),
                                schema_version: talos_sync_types::SCHEMA_VERSION,
                                protocol_version: talos_sync_types::PROTOCOL_VERSION,
                            },
                            SyncControlMessage::CatalogRequest(_) => {
                                SyncControlMessage::CatalogResponse(SyncPage {
                                    resources: vec![],
                                    next_cursor: None,
                                    feed_cursor: "cut".into(),
                                })
                            }
                            SyncControlMessage::Subscribe { .. } => {
                                SyncControlMessage::Subscribed {
                                    subscription_id: "feed".into(),
                                    feed_cursor: "cut".into(),
                                }
                            }
                            SyncControlMessage::Unsubscribe { .. } => continue,
                            other => panic!("unexpected control frame {other:?}"),
                        };
                        Message::Text(
                            serde_json::to_string(&SyncControlFrame {
                                request_id: request.request_id,
                                body,
                            })
                            .unwrap()
                            .into(),
                        )
                    }
                    Message::Close(_) => return,
                    other => panic!("unexpected frame {other:?}"),
                };
                socket.send(response).await.unwrap();
            }
        }
    });
    let fixture = fixture_with_options(
        SyncOptions {
            hub_endpoint: endpoint,
        },
        "hub-1",
    )
    .await;
    let (attempts, mut received) = mpsc::unbounded_channel();
    let mut attempt = 0;
    let attempts = std::sync::Mutex::new(move || {
        attempt += 1;
        attempts.send(Instant::now()).unwrap();
        attempt
    });
    let handle = Arc::new(fixture.sync)
        .start(
            Arc::new(EmptyAdapter),
            SyncStartParams {
                binding: fixture.binding,
                credentials: Arc::new(move || {
                    let attempt = attempts.lock().unwrap()();
                    Box::pin(async move {
                        if attempt <= 2 || attempt == 4 {
                            return Err(talos_sync_types::SyncError::new(if attempt == 4 {
                                SyncErrorCode::Unauthenticated
                            } else {
                                SyncErrorCode::ServiceUnavailable
                            }));
                        }
                        Ok(talos_sync_sdk::VerifiedCredential {
                            account_id: "account-1".into(),
                            access_token: "refreshed-login".into(),
                        })
                    })
                }),
                trusted_ca_pem: Some(trusted_ca),
                reconnect_delay: Duration::from_millis(100),
            },
        )
        .unwrap();
    handle.set_presence_computers(vec![]).unwrap();
    let first = received.recv().await.unwrap();
    let second = received.recv().await.unwrap();
    let third = received.recv().await.unwrap();
    assert!(second.duration_since(first) >= Duration::from_millis(100));
    assert!(third.duration_since(second) >= Duration::from_millis(100));
    wait_connection(&handle, SyncConnectionState::Connected).await;
    let dropped_at = Instant::now();
    disconnect.send(()).await.unwrap();
    let fourth = tokio::time::timeout(Duration::from_secs(2), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(fourth.duration_since(dropped_at) >= Duration::from_millis(100));
    wait_connection(&handle, SyncConnectionState::Disconnected).await;
    assert_eq!(
        handle.status().last_error.unwrap().code,
        talos_error::ErrorCode::Unauthenticated
    );
    // A later callback supplies the refreshed login. It must recover through a
    // real TLS handshake, without a special wake or restarting the runner.
    tokio::time::pause();
    tokio::time::advance(Duration::from_millis(100)).await;
    tokio::time::resume();
    received.recv().await.unwrap();
    wait_connection(&handle, SyncConnectionState::Connected).await;
    // Advance only an already subscribed real connection, never bootstrap or
    // a failed authentication attempt. Resume before waiting on local I/O.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(30)).await;
    tokio::time::resume();
    let dropped_at = Instant::now();
    disconnect.send(()).await.unwrap();
    let after_stable = tokio::time::timeout(Duration::from_secs(2), received.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(after_stable.duration_since(dropped_at) >= Duration::from_millis(100));
    wait_connection(&handle, SyncConnectionState::Connected).await;
    handle.shutdown().await.unwrap();
    peer.await.unwrap();
}
