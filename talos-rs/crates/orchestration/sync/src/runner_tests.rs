use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Storage, StorageTransaction};

use super::{capture, fixture, node, COMPUTER};
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
    ) -> TalosResult<Vec<SyncChange>> {
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
    let resource = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Computer(node("Before"))),
        }],
    )
    .await;
    let cut = fixture
        .sync
        .flush_cut(&fixture.binding, &resource)
        .await
        .unwrap()
        .0
        .unwrap();
    let first = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 1)
        .await
        .unwrap()
        .unwrap();
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: Some(Box::new(SyncResourceSnapshot::Computer(node("Before")))),
            after: Box::new(SyncResourceSnapshot::Computer(node("Later"))),
        }],
    )
    .await;
    fixture.sync.acknowledge(&first).await.unwrap();
    assert!(!fixture
        .sync
        .has_pending_through(&fixture.binding, &resource, cut)
        .await
        .unwrap());
    let later = fixture
        .sync
        .export_pending(&fixture.binding, resource, 1)
        .await
        .unwrap()
        .unwrap();
    assert!(later.through_sequence > cut);
    assert!(talos_sync_sdk::version_includes(&later.version, &first.version).unwrap());
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
    let mut transaction = fixture.storage.begin_transaction().await.unwrap();
    let (version, changes) = fixture
        .sync
        .import_document(
            &mut *transaction,
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
    while let Some(batch) = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
    {
        assert!(talos_sync_sdk::version_includes(&batch.version, &version).unwrap());
        fixture.sync.acknowledge(&batch).await.unwrap();
        acknowledged_rows += 1;
    }
    assert!(acknowledged_rows > 1);
    assert!(fixture
        .sync
        .pending_resources(&fixture.binding, 32)
        .await
        .unwrap()
        .is_empty());
}
