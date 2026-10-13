use super::*;
use pretty_assertions::assert_eq;
use std::time::Duration;
use talos_error::TalosResult;
use talos_storage::StorageTransaction;

struct EmptyAdapter;
impl SyncDataAdapter for EmptyAdapter {
    async fn scan(&self, _: SyncScanParams) -> TalosResult<SyncScanPage> {
        Ok(SyncScanPage {
            resources: vec![],
            next_cursor: None,
        })
    }
    async fn export(
        &self,
        _: &mut dyn StorageTransaction,
        _: SyncExportParams,
    ) -> TalosResult<Vec<SyncChange>> {
        Ok(vec![])
    }
    async fn apply(&self, _: &mut dyn StorageTransaction, _: &SyncImportBatch) -> TalosResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn permanent_upload_rejection_preserves_outbox_without_starving_later_resources() {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    use futures_util::{SinkExt, StreamExt};
    use talos_sync_sdk::loro_protocol::{self, Permission, ProtocolMessage, UpdateStatusCode};
    use talos_sync_types::{SyncControlFrame, SyncControlMessage, SyncErrorTarget, SyncPage};
    use tokio_tungstenite::tungstenite::Message;
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
    let uploads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = uploads.clone();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let tls = tokio_rustls::TlsAcceptor::from(Arc::new(tls))
            .accept(tcp)
            .await
            .unwrap();
        let mut socket = tokio_tungstenite::accept_async(tls).await.unwrap();
        while let Some(message) = socket.next().await {
            let response = match message.unwrap() {
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
                        SyncControlMessage::Subscribe { .. } => SyncControlMessage::Subscribed {
                            subscription_id: "feed".into(),
                            feed_cursor: "cut".into(),
                        },
                        SyncControlMessage::Unsubscribe { .. } => continue,
                        _ => panic!("unexpected control request"),
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
                Message::Binary(bytes) => {
                    let response = match loro_protocol::decode(&bytes).unwrap() {
                        ProtocolMessage::JoinRequest { crdt, room_id, .. } => {
                            ProtocolMessage::JoinResponseOk {
                                crdt,
                                room_id,
                                permission: Permission::Write,
                                version: vec![],
                                extra: Some(
                                    serde_json::to_vec(&talos_sync_types::DocumentJoinAccepted {
                                        history_generation: 0,
                                    })
                                    .unwrap(),
                                ),
                            }
                        }
                        ProtocolMessage::DocUpdate {
                            crdt,
                            room_id,
                            batch_id,
                            updates,
                        } => {
                            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            // Payload size alone determines the controlled remote refusal.
                            let too_large = updates.iter().map(Vec::len).sum::<usize>() > 4096;
                            if too_large {
                                socket
                                    .send(Message::Text(
                                        serde_json::to_string(&SyncControlFrame {
                                            request_id: None,
                                            body: SyncControlMessage::Error {
                                                error: talos_sync_types::SyncError::new(
                                                    talos_sync_types::SyncErrorCode::UploadTooLarge,
                                                ),
                                                target: SyncErrorTarget::Document {
                                                    room_id: room_id.clone(),
                                                    batch_id: batch_id.0,
                                                },
                                            },
                                        })
                                        .unwrap()
                                        .into(),
                                    ))
                                    .await
                                    .unwrap();
                            }
                            ProtocolMessage::Ack {
                                crdt,
                                room_id,
                                ref_id: batch_id,
                                status: if too_large {
                                    UpdateStatusCode::PayloadTooLarge
                                } else {
                                    UpdateStatusCode::Ok
                                },
                            }
                        }
                        ProtocolMessage::Leave { .. } => continue,
                        _ => panic!("unexpected document frame"),
                    };
                    Message::Binary(loro_protocol::encode(&response).unwrap().into())
                }
                Message::Close(_) => break,
                other => panic!("unexpected frame {other:?}"),
            };
            socket.send(response).await.unwrap();
        }
    });
    let fixture = super::fixture_with_options(
        crate::SyncOptions {
            hub_endpoint: endpoint,
        },
        "hub-1",
    )
    .await;
    let binding = fixture.binding.clone();
    let failed = SyncResourceKey::Computer {
        computer_id: uuid::Uuid::new_v4().to_string(),
    };
    // Nine rejected resources exceed one upload page; later healthy resources must still publish.
    let mut transaction = fixture.storage.begin_transaction().await.unwrap();
    let local_failure = SyncResourceKey::Computer {
        computer_id: uuid::Uuid::new_v4().to_string(),
    };
    let mut original = node("Invalid local binding");
    original.node_id = local_failure.id().into();
    let mut invalid = original.clone();
    invalid.owner_user_id = None;
    // Both captures fit the account scope, but forming one document must reject
    // the immutable owner change. Other resources must still be published.
    fixture
        .sync
        .capture(
            &mut *transaction,
            SyncCaptureParams {
                binding: binding.clone(),
                resource: local_failure.clone(),
                changes: vec![
                    SyncChange::Metadata {
                        before: None,
                        after: Box::new(SyncResourceSnapshot::Computer(original.clone())),
                    },
                    SyncChange::Metadata {
                        before: Some(Box::new(SyncResourceSnapshot::Computer(original))),
                        after: Box::new(SyncResourceSnapshot::Computer(invalid)),
                    },
                ],
                created_at: 1,
            },
        )
        .await
        .unwrap();
    for index in 0..10 {
        let resource = if index == 0 {
            failed.clone()
        } else {
            SyncResourceKey::Computer {
                computer_id: uuid::Uuid::new_v4().to_string(),
            }
        };
        let mut info = node(&if index < 9 {
            "x".repeat(12000)
        } else {
            "Healthy".into()
        });
        info.node_id = resource.id().into();
        fixture
            .sync
            .capture(
                &mut *transaction,
                SyncCaptureParams {
                    binding: binding.clone(),
                    resource,
                    changes: vec![SyncChange::Metadata {
                        before: None,
                        after: Box::new(SyncResourceSnapshot::Computer(info)),
                    }],
                    created_at: 1,
                },
            )
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
    let sync = Arc::new(fixture.sync);
    let handle = sync
        .clone()
        .start(
            Arc::new(EmptyAdapter),
            SyncStartParams {
                binding: binding.clone(),
                credentials: Arc::new(|| {
                    Box::pin(async {
                        Ok(talos_sync_sdk::VerifiedCredential {
                            account_id: "account-1".into(),
                            access_token: "controlled-test".into(),
                        })
                    })
                }),
                trusted_ca_pem: Some(cert.pem().into_bytes()),
                reconnect_delay: Duration::from_millis(20),
            },
        )
        .unwrap();
    let mut status = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if status.borrow().last_acknowledged_at.is_some() {
                break;
            }
            status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(handle.status().connection, SyncConnectionState::Connected);
    assert!(matches!(
        handle.status().last_error.unwrap().code,
        ErrorCode::UploadTooLarge | ErrorCode::Conflict
    ));
    assert_eq!(
        sync.pending_resources(&binding, 32).await.unwrap().len(),
        10
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        uploads.load(std::sync::atomic::Ordering::SeqCst),
        10,
        "Permanent rejections must not automatically retry unchanged facts"
    );
    assert_eq!(
        handle.flush(failed).await.unwrap_err().error_code(),
        ErrorCode::UploadTooLarge
    );
    assert_eq!(
        handle.flush(local_failure).await.unwrap_err().error_code(),
        ErrorCode::Conflict
    );
    assert_eq!(handle.status().connection, SyncConnectionState::Connected);
    handle.shutdown().await.unwrap();
    peer.await.unwrap();
}
