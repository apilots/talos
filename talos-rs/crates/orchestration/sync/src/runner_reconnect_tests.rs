use super::{fixture, fixture_with_options, runner_wss::tls_listener, runner_wss::EmptyAdapter};
use crate::*;
use futures_util::{SinkExt, StreamExt};
use pretty_assertions::assert_eq;
use std::{sync::Arc, time::Duration};
use talos_sync_types::{SyncControlFrame, SyncControlMessage, SyncErrorCode, SyncPage};
use tokio::{sync::mpsc, time::Instant};
use tokio_tungstenite::tungstenite::Message;

struct DeferredImport {
    resource: SyncResourceKey,
    released: Arc<std::sync::atomic::AtomicBool>,
}

impl SyncDataAdapter for DeferredImport {
    async fn scan(&self, params: SyncScanParams) -> talos_error::TalosResult<SyncScanPage> {
        EmptyAdapter.scan(params).await
    }

    async fn export(
        &self,
        tx: &mut dyn talos_storage::StorageTransaction,
        params: SyncExportParams,
    ) -> talos_error::TalosResult<SyncExportFacts> {
        EmptyAdapter.export(tx, params).await
    }

    async fn apply(
        &self,
        _: &mut dyn talos_storage::StorageTransaction,
        batch: &SyncImportBatch,
    ) -> talos_error::TalosResult<()> {
        if batch.resource == self.resource
            && !self.released.load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Local facts are still pending capture.",
            ));
        }
        Ok(())
    }
}

#[tokio::test]
async fn deferred_import_replays_from_the_gap_without_blocking_later_feed_entries() {
    use talos_storage::Storage;
    use talos_sync_sdk::loro_protocol::{self, BatchId, Permission, ProtocolMessage};
    use talos_sync_types::{DocumentJoinAccepted, SyncFeedEntry};

    let (listener, acceptor, trusted_ca) = tls_listener().await;
    let fixture = fixture_with_options(
        SyncOptions {
            hub_endpoint: format!(
                "wss://localhost:{}/v1/sync",
                listener.local_addr().unwrap().port()
            ),
        },
        "hub-1",
    )
    .await;
    fixture
        .storage
        .execute("UPDATE sync_bindings SET download_cursor='base'", &[])
        .await
        .unwrap();

    let mut documents = std::collections::HashMap::new();
    let mut entries = Vec::new();
    for (writer, name) in [(100, "Deferred"), (200, "Healthy")] {
        let resource = SyncResourceKey::Computer {
            computer_id: uuid::Uuid::new_v4().to_string(),
        };
        let mut metadata = super::node(name);
        metadata.node_id = resource.id().into();
        let change = crate::projection::change(
            &fixture.binding,
            SyncChange::Metadata {
                before: None,
                after: Box::new(SyncResourceSnapshot::Computer(metadata)),
            },
        )
        .unwrap()
        .unwrap();
        let mut replica = talos_sync_sdk::SyncReplica::new(resource.clone(), writer).unwrap();
        let update = replica.apply(&[change]).unwrap();
        let version = replica.version().unwrap();
        let room = talos_sync_types::room_id(&fixture.binding.scope, &resource).unwrap();
        documents.insert(room, (version.clone(), update));
        entries.push(SyncFeedEntry {
            resource,
            version,
            deleted: false,
            reset: false,
        });
    }
    let deferred = entries[0].resource.clone();
    let healthy = entries[1].resource.clone();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let tls = acceptor.accept(tcp).await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tls).await.unwrap();
        let control = |request_id, body| {
            Message::Text(
                serde_json::to_string(&SyncControlFrame { request_id, body })
                    .unwrap()
                    .into(),
            )
        };
        let binary = |body| Message::Binary(loro_protocol::encode(&body).unwrap().into());
        while let Some(Ok(frame)) = socket.next().await {
            match frame {
                Message::Text(text) if text == "ping" => {
                    socket.send(Message::Text("pong".into())).await.unwrap()
                }
                Message::Text(text) => {
                    let request: SyncControlFrame = serde_json::from_str(&text).unwrap();
                    match request.body {
                        SyncControlMessage::Hello { .. } => socket
                            .send(control(
                                request.request_id,
                                SyncControlMessage::HelloAccepted {
                                    hub_id: "hub-1".into(),
                                    schema_version: talos_sync_types::SCHEMA_VERSION,
                                    protocol_version: talos_sync_types::PROTOCOL_VERSION,
                                },
                            ))
                            .await
                            .unwrap(),
                        SyncControlMessage::Subscribe {
                            after_feed_cursor, ..
                        } => {
                            assert_eq!(after_feed_cursor.as_deref(), Some("base"));
                            socket
                                .send(control(
                                    request.request_id,
                                    SyncControlMessage::Subscribed {
                                        subscription_id: "feed".into(),
                                        feed_cursor: "base".into(),
                                    },
                                ))
                                .await
                                .unwrap();
                            for (entry, cursor) in entries.iter().zip(["one", "two"]) {
                                socket
                                    .send(control(
                                        None,
                                        SyncControlMessage::FeedBatch {
                                            subscription_id: "feed".into(),
                                            entries: vec![entry.clone()],
                                            next_cursor: cursor.into(),
                                        },
                                    ))
                                    .await
                                    .unwrap();
                            }
                        }
                        unsubscribe @ SyncControlMessage::Unsubscribe { .. } => {
                            socket
                                .send(control(request.request_id, unsubscribe))
                                .await
                                .unwrap();
                        }
                        other => panic!("Unexpected control frame: {other:?}"),
                    }
                }
                Message::Binary(bytes) => match loro_protocol::decode(&bytes).unwrap() {
                    ProtocolMessage::JoinRequest { crdt, room_id, .. }
                        if crdt == loro_protocol::CrdtType::Loro =>
                    {
                        let (version, update) = documents.get(&room_id).unwrap();
                        socket
                            .send(binary(ProtocolMessage::JoinResponseOk {
                                crdt,
                                room_id: room_id.clone(),
                                permission: Permission::Write,
                                version: version.version_vector.clone(),
                                extra: Some(
                                    serde_json::to_vec(&DocumentJoinAccepted {
                                        history_generation: version.history_generation,
                                    })
                                    .unwrap(),
                                ),
                            }))
                            .await
                            .unwrap();
                        socket
                            .send(binary(ProtocolMessage::DocUpdate {
                                crdt,
                                room_id,
                                batch_id: BatchId([1; 8]),
                                updates: vec![update.clone()],
                            }))
                            .await
                            .unwrap();
                    }
                    ProtocolMessage::JoinRequest { .. }
                    | ProtocolMessage::Leave { .. }
                    | ProtocolMessage::Ack { .. } => {}
                    other => panic!("Unexpected document frame: {other:?}"),
                },
                Message::Close(_) => break,
                other => panic!("Unexpected frame: {other:?}"),
            }
        }
    });
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sync = Arc::new(fixture.sync);
    let handle = sync
        .clone()
        .start(
            Arc::new(DeferredImport {
                resource: deferred.clone(),
                released: released.clone(),
            }),
            SyncStartParams {
                binding: fixture.binding.clone(),
                credentials: Arc::new(|| {
                    Box::pin(async {
                        Ok(talos_sync_sdk::VerifiedCredential {
                            account_id: "account-1".into(),
                            access_token: "test-credential".into(),
                        })
                    })
                }),
                trusted_ca_pem: Some(trusted_ca),
                reconnect_delay: Duration::from_secs(30),
            },
        )
        .unwrap();
    let mut changes = handle.subscribe_changes();
    tokio::time::timeout(Duration::from_secs(3), async {
        assert_eq!(changes.recv().await.unwrap().resource, healthy);
        assert_eq!(
            sync.download_cursor(&fixture.binding)
                .await
                .unwrap()
                .as_deref(),
            Some("base")
        );
        assert!(sync
            .document_version(&fixture.binding, &deferred)
            .await
            .unwrap()
            .is_none());
        released.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(changes.recv().await.unwrap().resource, deferred);
        while sync
            .download_cursor(&fixture.binding)
            .await
            .unwrap()
            .as_deref()
            != Some("two")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    handle.shutdown().await.unwrap();
    peer.await.unwrap();
}

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
async fn rejected_attempts_cool_down_and_transient_failures_keep_the_configured_interval() {
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
    for next_error in [
        SyncErrorCode::PermissionDenied,
        SyncErrorCode::MessageQueueFull,
        SyncErrorCode::ServiceUnavailable,
    ] {
        errors.send_replace(next_error);
        // Local commits must not bypass the rejection cooldown.
        handle.notify_committed();
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(9)).await;
        tokio::time::resume();
        // Let local SQLite I/O finish if an incorrect early retry started.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(received.try_recv().is_err());
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::time::resume();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), received.recv())
                .await
                .unwrap(),
            Some(next_error)
        );
        wait_connection(&handle, SyncConnectionState::Disconnected).await;
        // No SDK-internal retry may run inside one durable attempt.
        assert!(received.try_recv().is_err());
    }
    errors.send_replace(SyncErrorCode::Unauthenticated);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::resume();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), received.recv())
            .await
            .unwrap(),
        Some(SyncErrorCode::Unauthenticated)
    );
    wait_connection(&handle, SyncConnectionState::Disconnected).await;
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
        let mut subscribed = false;
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
                                assert!(
                                    !subscribed,
                                    "a valid saved cursor must skip catalog recovery"
                                );
                                SyncControlMessage::CatalogResponse(SyncPage {
                                    resources: vec![],
                                    next_cursor: None,
                                    feed_cursor: "cut".into(),
                                })
                            }
                            SyncControlMessage::Subscribe {
                                after_feed_cursor, ..
                            } => {
                                assert_eq!(after_feed_cursor.as_deref(), Some("cut"));
                                subscribed = true;
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
    tokio::time::advance(Duration::from_secs(10)).await;
    tokio::time::resume();
    let fifth = received.recv().await.unwrap();
    assert!(fifth.duration_since(fourth) >= Duration::from_secs(10));
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
