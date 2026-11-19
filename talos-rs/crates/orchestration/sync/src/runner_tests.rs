use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Storage, StorageTransaction};
use tokio::sync::Notify;

use super::{capture, fixture, node, options, COMPUTER};
use crate::*;

#[tokio::test]
async fn claimed_embedded_computer_and_session_use_the_same_permanent_uuid() {
    let fixture = fixture().await;
    let source_key = SyncResourceKey::Computer {
        computer_id: "embedded".into(),
    };
    let mut local_node = node("Local computer");
    local_node.owner_user_id = None;
    capture(
        &fixture,
        source_key.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Computer(local_node)),
        }],
    )
    .await;
    let computer = fixture
        .sync
        .export_pending(&fixture.binding, source_key, 32)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        computer.resource,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into()
        }
    );
    let mut remote = talos_sync_sdk::SyncReplica::new(computer.resource.clone(), 41).unwrap();
    remote.import(&[computer.update]).unwrap();
    let facts = remote.changes().unwrap();
    let talos_sync_types::SyncChange::Metadata { after, .. } = &facts[0] else {
        panic!("Computer metadata")
    };
    let talos_sync_types::SyncResourceSnapshot::Computer(node) = after.as_ref() else {
        panic!("Computer metadata")
    };
    assert_eq!(node.node_id, COMPUTER);
    assert_eq!(node.owner_user_id.as_deref(), Some("account-1"));

    let (session, _) = super::transcript::session_and_events("embedded");
    let original_binding = session.binding.clone();
    let resource = SyncResourceKey::Session {
        session_id: session.session.id.clone(),
    };
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Session(Box::new(session))),
        }],
    )
    .await;
    let batch = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 32)
        .await
        .unwrap()
        .unwrap();
    let mut remote = talos_sync_sdk::SyncReplica::new(resource, 42).unwrap();
    remote.import(&[batch.update]).unwrap();
    let facts = remote.changes().unwrap();
    let talos_sync_types::SyncChange::Metadata { after, .. } = &facts[0] else {
        panic!("Session metadata")
    };
    let talos_sync_types::SyncResourceSnapshot::Session(session) = after.as_ref() else {
        panic!("Session metadata")
    };
    assert_eq!(session.session.node_id, COMPUTER);
    assert_eq!(session.binding.computer_id, COMPUTER);
    assert_eq!(session.binding.agent_id, original_binding.agent_id);
    assert_eq!(session.binding.workspace_id, original_binding.workspace_id);
    assert_eq!(
        session.binding.workspace_user_id,
        original_binding.workspace_user_id
    );
}

struct DisabledAdapter(Arc<AtomicUsize>);

impl SyncDataAdapter for DisabledAdapter {
    async fn scan(&self, _: SyncScanParams) -> TalosResult<SyncScanPage> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(TalosError::internal(
            "Disabled synchronization must not scan business data.",
        ))
    }
    async fn export(
        &self,
        _: &mut dyn StorageTransaction,
        _: SyncExportParams,
    ) -> TalosResult<SyncExportFacts> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(TalosError::internal(
            "Disabled synchronization must not export business data.",
        ))
    }
    async fn apply(&self, _: &mut dyn StorageTransaction, _: &SyncImportBatch) -> TalosResult<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(TalosError::internal(
            "Disabled synchronization must not import business data.",
        ))
    }
}

struct ReconnectResources {
    queued: SyncResourceKey,
    published: SyncResourceKey,
    deferred: SyncResourceKey,
}

impl SyncDataAdapter for ReconnectResources {
    async fn scan(&self, _: SyncScanParams) -> TalosResult<SyncScanPage> {
        Ok(SyncScanPage {
            resources: vec![
                self.queued.clone(),
                self.published.clone(),
                self.deferred.clone(),
            ],
            next_cursor: None,
        })
    }

    async fn export(
        &self,
        _: &mut dyn StorageTransaction,
        params: SyncExportParams,
    ) -> TalosResult<SyncExportFacts> {
        assert_eq!(params.resource, self.deferred);
        Ok(SyncExportFacts::default())
    }

    async fn apply(&self, _: &mut dyn StorageTransaction, _: &SyncImportBatch) -> TalosResult<()> {
        panic!("the test stops at the credential callback")
    }
}

#[tokio::test]
async fn reconnect_checks_known_and_deferred_resources_without_taking_the_writer() {
    let fixture = fixture().await;
    let queued = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    capture(
        &fixture,
        queued.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Computer(node("Queued"))),
        }],
    )
    .await;
    let (session, _) = super::transcript::session_and_events("embedded");
    let published = SyncResourceKey::Session {
        session_id: session.session.id.clone(),
    };
    capture(
        &fixture,
        published.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Session(Box::new(session))),
        }],
    )
    .await;
    let batch = fixture
        .sync
        .export_pending(&fixture.binding, published.clone(), 1)
        .await
        .unwrap()
        .unwrap();
    fixture.sync.acknowledge(&batch).await.unwrap();

    let writer = fixture.storage.begin_transaction().await.unwrap();
    let actor = talos_auth::RequestIdentity {
        user_id: fixture.binding.scope.account_id.clone(),
        username: None,
        role: talos_auth::UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    };
    let rebound = tokio::time::timeout(
        Duration::from_secs(2),
        fixture.sync.bind_account(
            &actor,
            SyncBindParams {
                scope: fixture.binding.scope.clone(),
                source_user_id: fixture.binding.source_user_id.clone(),
                computers: fixture.binding.computers.clone(),
            },
        ),
    )
    .await
    .expect("Reusing the unchanged binding must not queue for the writer")
    .unwrap();
    assert_eq!(rebound, fixture.binding);
    let credentials_requested = Arc::new(tokio::sync::Notify::new());
    let observed = credentials_requested.clone();
    let handle = Arc::new(fixture.sync)
        .start(
            Arc::new(ReconnectResources {
                queued,
                published,
                deferred: SyncResourceKey::Computer {
                    computer_id: "00000000-0000-4000-8000-000000000004".into(),
                },
            }),
            SyncStartParams {
                binding: fixture.binding,
                credentials: Arc::new(move || {
                    observed.notify_one();
                    Box::pin(async {
                        Err(talos_sync_types::SyncError::new(
                            talos_sync_types::SyncErrorCode::Unauthenticated,
                        ))
                    })
                }),
                trusted_ca_pem: None,
                reconnect_delay: Duration::from_secs(10),
            },
        )
        .unwrap();
    let connected =
        tokio::time::timeout(Duration::from_secs(2), credentials_requested.notified()).await;
    handle.shutdown().await.unwrap();
    writer.rollback().await.unwrap();
    connected.expect("reconnect must request credentials while the SQLite writer is occupied");
}

struct DeferredBaseline {
    resource: SyncResourceKey,
    projecting: Notify,
    resume: Notify,
}

impl SyncDataAdapter for DeferredBaseline {
    async fn scan(&self, _: SyncScanParams) -> TalosResult<SyncScanPage> {
        Ok(SyncScanPage {
            resources: vec![self.resource.clone()],
            next_cursor: None,
        })
    }

    async fn export(
        &self,
        _: &mut dyn StorageTransaction,
        _: SyncExportParams,
    ) -> TalosResult<SyncExportFacts> {
        self.projecting.notify_one();
        self.resume.notified().await;
        Ok(SyncExportFacts {
            changes: vec![SyncChange::Metadata {
                before: None,
                after: Box::new(SyncResourceSnapshot::Computer(node("Initial projection"))),
            }],
            ..Default::default()
        })
    }

    async fn apply(&self, _: &mut dyn StorageTransaction, _: &SyncImportBatch) -> TalosResult<()> {
        panic!("the test stops at the credential callback")
    }
}

#[tokio::test]
async fn local_capture_wins_a_race_with_the_initial_projection() {
    let fixture = fixture().await;
    let resource = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    let adapter = Arc::new(DeferredBaseline {
        resource: resource.clone(),
        projecting: Notify::new(),
        resume: Notify::new(),
    });
    let credentials_requested = Arc::new(Notify::new());
    let observed = credentials_requested.clone();
    // The fixture retains the original facade for the foreground commit. Both
    // facades share the same durable binding and SQLite writer.
    let runner_sync = Arc::new(
        TalosSync::open(fixture.storage.clone(), options())
            .await
            .unwrap(),
    );
    let handle = runner_sync
        .start(
            adapter.clone(),
            SyncStartParams {
                binding: fixture.binding.clone(),
                credentials: Arc::new(move || {
                    observed.notify_one();
                    Box::pin(async {
                        Err(talos_sync_types::SyncError::new(
                            talos_sync_types::SyncErrorCode::Unauthenticated,
                        ))
                    })
                }),
                trusted_ca_pem: None,
                reconnect_delay: Duration::from_secs(10),
            },
        )
        .unwrap();
    let timeout = Duration::from_secs(2);
    tokio::time::timeout(timeout, adapter.projecting.notified())
        .await
        .expect("initial projection must start");
    let committed = tokio::time::timeout(
        timeout,
        capture(
            &fixture,
            resource.clone(),
            vec![SyncChange::Metadata {
                before: None,
                after: Box::new(SyncResourceSnapshot::Computer(node("Foreground commit"))),
            }],
        ),
    )
    .await;
    adapter.resume.notify_one();
    let connected = tokio::time::timeout(timeout, credentials_requested.notified()).await;
    handle.shutdown().await.unwrap();
    committed.expect("foreground capture must commit before projection resumes");
    connected.expect("runner must finish the initial projection");

    let rows = fixture
        .storage
        .query(
            "SELECT changes_json FROM sync_outbox WHERE resource_id = ?",
            &[resource.id()],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let changes: Vec<talos_sync_types::SyncChange> =
        serde_json::from_str(&rows[0].get_string("changes_json").unwrap()).unwrap();
    let [talos_sync_types::SyncChange::Metadata {
        before: None,
        after,
    }] = changes.as_slice()
    else {
        panic!("expected one initial Computer snapshot");
    };
    let talos_sync_types::SyncResourceSnapshot::Computer(computer) = after.as_ref() else {
        panic!("expected Computer metadata");
    };
    assert_eq!(computer.display_name, "Foreground commit");
}

#[tokio::test]
async fn disabled_binding_stops_before_business_scan_or_authentication() {
    let fixture = fixture().await;
    fixture
        .sync
        .set_enabled(&fixture.binding.scope.account_id, false)
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = calls.clone();
    let sync = Arc::new(fixture.sync);
    let handle = sync
        .start(
            Arc::new(DisabledAdapter(calls.clone())),
            SyncStartParams {
                binding: fixture.binding,
                credentials: Arc::new(move || {
                    callback_calls.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async {
                        Err(talos_sync_types::SyncError::new(
                            talos_sync_types::SyncErrorCode::Unauthenticated,
                        ))
                    })
                }),
                trusted_ca_pem: None,
                reconnect_delay: Duration::from_millis(10),
            },
        )
        .unwrap();
    let mut status = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(2), async {
        while status.borrow().connection != SyncConnectionState::Disabled {
            status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let error = handle
        .flush(SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::ServiceUnavailable);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn fixed_flush_cut_does_not_include_later_committed_changes() {
    let fixture = fixture().await;
    let (session, _) = super::transcript::session_and_events("embedded");
    let resource = SyncResourceKey::Session {
        session_id: session.session.id.clone(),
    };
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Session(Box::new(session))),
        }],
    )
    .await;
    capture(&fixture, resource.clone(), vec![SyncChange::Delete]).await;
    fixture.storage.execute(
        "INSERT INTO sync_resource_capture(hub_id, account_id, kind, resource_id, revision, outbox_sequence)
         SELECT hub_id, account_id, kind, resource_id, seq, seq FROM sync_outbox WHERE kind='session'",
        &[],
    ).await.unwrap();
    let captured = fixture
        .storage
        .query(
            "SELECT MIN(outbox_sequence) AS outbox_sequence FROM sync_resource_capture",
            &[],
        )
        .await
        .unwrap()[0]
        .get_i64("outbox_sequence")
        .unwrap();
    let latest = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let minimum = fixture
        .sync
        .captured_version(&fixture.binding, &resource, captured)
        .await
        .unwrap();
    assert!(!talos_sync_sdk::version_includes(&minimum, &latest.version).unwrap());

    let first = fixture
        .sync
        .export_pending_through(&fixture.binding, resource.clone(), 128, captured)
        .await
        .unwrap()
        .unwrap();
    let mut hub = talos_sync_sdk::SyncReplica::new(resource.clone(), 42).unwrap();
    hub.import(std::slice::from_ref(&first.update)).unwrap();
    assert!(talos_sync_sdk::version_equal(&hub.version().unwrap(), &minimum).unwrap());
    assert!(!hub.summary().unwrap().deleted);

    fixture.sync.acknowledge(&first).await.unwrap();
    assert!(!fixture
        .sync
        .has_pending_through(&fixture.binding, &resource, captured)
        .await
        .unwrap());
    let after_ack = fixture
        .sync
        .captured_version(&fixture.binding, &resource, captured)
        .await
        .unwrap();
    assert!(talos_sync_sdk::version_equal(&minimum, &after_ack).unwrap());

    let later = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let mut next =
        talos_sync_sdk::SyncReplica::from_snapshot(resource.clone(), &hub.snapshot().unwrap())
            .unwrap();
    next.import(&[later.update]).unwrap();
    assert!(next.summary().unwrap().deleted);

    hub.set_writer_id(42).unwrap();
    let remote_delete = hub.apply(&[talos_sync_types::SyncChange::Delete]).unwrap();
    let hub_version = hub.version().unwrap();
    let (transaction, _, _) = fixture
        .sync
        .import_document(
            &fixture.binding,
            &resource,
            &[remote_delete],
            &hub_version,
            None,
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert!(!fixture
        .sync
        .has_pending_through(&fixture.binding, &resource, later.through_sequence)
        .await
        .unwrap());
    let superseded = fixture
        .sync
        .captured_version(&fixture.binding, &resource, later.through_sequence)
        .await
        .unwrap();
    assert!(talos_sync_sdk::version_equal(&later.version, &superseded).unwrap());
    assert!(!talos_sync_sdk::version_includes(&hub_version, &superseded).unwrap());
}

#[tokio::test]
async fn offline_backlog_larger_than_one_thousand_rows_recovers_before_remote_import() {
    let fixture = fixture().await;
    let resource = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    let mut transaction = fixture.storage.begin_transaction().await.unwrap();
    for index in 0..1050 {
        fixture
            .sync
            .capture(
                &mut *transaction,
                SyncCaptureParams {
                    binding: fixture.binding.clone(),
                    resource: resource.clone(),
                    changes: vec![SyncChange::Metadata {
                        before: (index > 0).then(|| {
                            Box::new(SyncResourceSnapshot::Computer(node(&format!(
                                "offline-{}",
                                index - 1
                            ))))
                        }),
                        after: Box::new(SyncResourceSnapshot::Computer(node(&format!(
                            "offline-{index}"
                        )))),
                    }],
                    created_at: index,
                },
            )
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
    fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let (transaction, version, changes) = fixture
        .sync
        .import_document(
            &fixture.binding,
            &resource,
            &[],
            &SyncVersion::default(),
            None,
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let talos_sync_types::SyncChange::Metadata { after, .. } = &changes[0] else {
        panic!("Computer metadata")
    };
    let talos_sync_types::SyncResourceSnapshot::Computer(node) = after.as_ref() else {
        panic!("Computer metadata")
    };
    assert_eq!(node.display_name, "offline-1049");
    let mut acknowledged_rows = 0;
    let mut hub = talos_sync_sdk::SyncReplica::new(resource.clone(), 42).unwrap();
    while let Some(batch) = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
    {
        hub.import(std::slice::from_ref(&batch.update)).unwrap();
        assert!(talos_sync_sdk::version_equal(&hub.version().unwrap(), &batch.version).unwrap());
        fixture.sync.acknowledge(&batch).await.unwrap();
        acknowledged_rows += 1;
    }
    assert!(acknowledged_rows > 1);
    assert!(talos_sync_sdk::version_equal(&hub.version().unwrap(), &version).unwrap());
    assert!(fixture
        .sync
        .pending_resources(&fixture.binding, 32)
        .await
        .unwrap()
        .is_empty());
}
