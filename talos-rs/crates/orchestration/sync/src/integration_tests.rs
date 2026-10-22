use std::{path::Path, sync::Arc};

use pretty_assertions::assert_eq;
use talos_auth::{RequestIdentity, UserRole};
use talos_error::ErrorCode;
use talos_storage::{SqliteStorage, Storage};
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_sync_sdk::SyncReplica;
use talos_types::{NodeHealth, NodeInfo, NodeMode, NodeStatus, ProjectInfo};

use crate::*;

#[path = "transcript_fixture.rs"]
mod transcript;

#[path = "runner_wss_tests.rs"]
mod runner_wss;

#[path = "runner_tests.rs"]
mod runner;

#[path = "deletion_tests.rs"]
mod deletion;

#[path = "version_tests.rs"]
mod version;

const COMPUTER: &str = "d4383755-33d1-43fd-a6ef-cf14dc2a823d";

struct Fixture {
    storage: Arc<SqliteStorage>,
    sync: TalosSync,
    binding: SyncBinding,
    _home: tempfile::TempDir,
}

fn options() -> SyncOptions {
    SyncOptions {
        hub_endpoint: "wss://sync.example.test/v1/sync".into(),
    }
}

async fn fixture() -> Fixture {
    fixture_with_options(options(), "hub-1").await
}

async fn fixture_with_options(options: SyncOptions, hub_id: &str) -> Fixture {
    let home = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        SqliteStorage::new(home.path().join("talos.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let sync = TalosSync::open(storage.clone(), options).await.unwrap();
    let actor = RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        auth_disabled: false,
        access_token: None,
    };
    let binding = sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: SyncScope {
                    hub_id: hub_id.into(),
                    account_id: actor.user_id.clone(),
                },
                source_user_id: actor.user_id.clone(),
                computers: vec![ComputerAlias {
                    source_node_id: "embedded".into(),
                    computer_id: COMPUTER.into(),
                }],
            },
        )
        .await
        .unwrap();
    Fixture {
        storage,
        sync,
        binding,
        _home: home,
    }
}

fn node(name: &str) -> NodeInfo {
    NodeInfo {
        system_info: None,
        node_id: "embedded".into(),
        display_name: name.into(),
        mode: NodeMode::Embedded,
        endpoint: None,
        status: NodeStatus::Enabled,
        health: NodeHealth::Healthy,
        lease_expires_at: Some(30),
        last_heartbeat_at: Some(10),
        created_at: 1,
        updated_at: 10,
        owner_user_id: Some("account-1".into()),
        available: true,
        credential_expires_at: Some(90),
    }
}

async fn capture(fixture: &Fixture, resource: SyncResourceKey, changes: Vec<SyncChange>) {
    let mut tx = fixture.storage.begin_transaction().await.unwrap();
    fixture
        .sync
        .capture(
            &mut *tx,
            SyncCaptureParams {
                binding: fixture.binding.clone(),
                resource,
                changes,
                created_at: 100,
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn business_rollback_removes_capture_and_commit_exports_exact_project_change() {
    let fixture = fixture().await;
    let projects = StorageProjectStore::new(fixture.storage.clone())
        .await
        .unwrap();
    let project = projects
        .create_project_on_node("Before", Path::new("/worker/work"), "account-1", "embedded")
        .await
        .unwrap();
    let before = ProjectInfo {
        node_id: project.execution_node_id,
        project_id: project.project_id,
        name: project.name,
        directory: project.directory,
        status: project.status,
        created_at: project.created_at,
        updated_at: project.updated_at,
        session_count: 0,
    };
    let mut after = before.clone();
    after.name = "After".into();
    let resource = SyncResourceKey::Project {
        project_id: before.project_id.clone(),
    };
    let changes = || {
        vec![SyncChange::Metadata {
            before: Some(Box::new(SyncResourceSnapshot::Project(before.clone()))),
            after: Box::new(SyncResourceSnapshot::Project(after.clone())),
        }]
    };
    let mut tx = fixture.storage.begin_transaction().await.unwrap();
    tx.execute(
        "UPDATE projects SET name = ? WHERE project_id = ?",
        &[&after.name, &before.project_id],
    )
    .await
    .unwrap();
    fixture
        .sync
        .capture(
            &mut *tx,
            SyncCaptureParams {
                binding: fixture.binding.clone(),
                resource: resource.clone(),
                changes: changes(),
                created_at: 100,
            },
        )
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        projects
            .get_project(&before.project_id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Before"
    );
    assert!(fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 32)
        .await
        .unwrap()
        .is_none());
    // Bootstrap establishes all immutable fields; the patch then changes only name.
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Project(before.clone())),
        }],
    )
    .await;
    let mut tx = fixture.storage.begin_transaction().await.unwrap();
    tx.execute(
        "UPDATE projects SET name = ? WHERE project_id = ?",
        &[&after.name, &before.project_id],
    )
    .await
    .unwrap();
    fixture
        .sync
        .capture(
            &mut *tx,
            SyncCaptureParams {
                binding: fixture.binding.clone(),
                resource: resource.clone(),
                changes: changes(),
                created_at: 101,
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let batch = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 32)
        .await
        .unwrap()
        .unwrap();
    let mut received = SyncReplica::new(resource, 99).unwrap();
    received.import(&[batch.update]).unwrap();
    let snapshots = received.changes().unwrap();
    let talos_sync_types::SyncChange::Metadata { after, .. } = &snapshots[0] else {
        panic!("project metadata")
    };
    let talos_sync_types::SyncResourceSnapshot::Project(actual) = after.as_ref() else {
        panic!("project metadata")
    };
    assert_eq!(
        (&actual.name, &actual.node_id),
        (&"After".to_string(), &COMPUTER.to_string())
    );
    assert_eq!(
        projects
            .get_project(&before.project_id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "After"
    );
}

#[tokio::test]
async fn lost_ack_restart_reuses_identity_and_does_not_duplicate_facts() {
    let fixture = fixture().await;
    let resource = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Computer(node("Laptop"))),
        }],
    )
    .await;
    let batch = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 32)
        .await
        .unwrap()
        .unwrap();
    let restarted = TalosSync::open(fixture.storage.clone(), options())
        .await
        .unwrap();
    assert_eq!(restarted.peer_id(), fixture.sync.peer_id());
    let retry = restarted
        .export_pending(&fixture.binding, resource.clone(), 32)
        .await
        .unwrap()
        .unwrap();
    assert!(talos_sync_sdk::version_equal(&retry.version, &batch.version).unwrap());
    assert_eq!(retry.through_sequence, batch.through_sequence);
    let mut receiver = SyncReplica::new(resource.clone(), 50).unwrap();
    receiver
        .import(&[batch.update, retry.update.clone()])
        .unwrap();
    assert_eq!(receiver.changes().unwrap().len(), 1);
    // A pure ACK must not rewrite an unchanged document blob/version. Enforce
    // that persistence boundary in SQLite rather than mocking the encoder.
    fixture.storage.execute("CREATE TRIGGER ack_preserves_document BEFORE UPDATE OF snapshot_blob, version_vector, generation ON sync_documents BEGIN SELECT RAISE(ABORT, 'ACK rewrote the document'); END", &[]).await.unwrap();
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: Some(Box::new(SyncResourceSnapshot::Computer(node("Laptop")))),
            after: Box::new(SyncResourceSnapshot::Computer(node("Renamed laptop"))),
        }],
    )
    .await;
    let ack_before = fixture.storage.query("SELECT hex(acknowledged_vector) AS vector, acknowledged_generation FROM sync_documents", &[]).await.unwrap();
    fixture.storage.execute("CREATE TRIGGER ack_cleanup_failure BEFORE DELETE ON sync_outbox BEGIN SELECT RAISE(ABORT, 'controlled ACK cleanup failure'); END", &[]).await.unwrap();
    assert!(restarted.acknowledge(&retry).await.is_err());
    assert_eq!(fixture.storage.query("SELECT hex(acknowledged_vector) AS vector, acknowledged_generation FROM sync_documents", &[]).await.unwrap()[0].values, ack_before[0].values);
    assert_eq!(
        fixture
            .storage
            .query("SELECT COUNT(*) AS count FROM sync_outbox", &[])
            .await
            .unwrap()[0]
            .get_i64("count"),
        Some(2)
    );
    fixture
        .storage
        .execute("DROP TRIGGER ack_cleanup_failure", &[])
        .await
        .unwrap();
    restarted.acknowledge(&retry).await.unwrap();
    restarted.acknowledge(&retry).await.unwrap();
    fixture
        .storage
        .execute("DROP TRIGGER ack_preserves_document", &[])
        .await
        .unwrap();
    let next = restarted
        .export_pending(&fixture.binding, resource.clone(), 32)
        .await
        .unwrap()
        .expect("later unacknowledged facts must survive an older ACK");
    assert!(next.through_sequence > retry.through_sequence);
    receiver.import(std::slice::from_ref(&next.update)).unwrap();
    assert!(serde_json::to_string(&receiver.changes().unwrap())
        .unwrap()
        .contains("Renamed laptop"));
    restarted.acknowledge(&next).await.unwrap();
    assert!(restarted
        .export_pending(&fixture.binding, resource, 32)
        .await
        .unwrap()
        .is_none());
    let wire = serde_json::to_value(receiver.changes().unwrap()).unwrap();
    for forbidden in [
        "leaseExpiresAt",
        "lastHeartbeatAt",
        "health",
        "available",
        "credentialExpiresAt",
    ] {
        assert!(wire[0]["data"]["after"]["data"].get(forbidden).is_none());
    }
}

#[tokio::test]
async fn heartbeat_changes_do_not_capture_metadata_and_disabled_transport_keeps_local_edits() {
    let fixture = fixture().await;
    let resource = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    let before = node("Laptop");
    let mut after = before.clone();
    after.updated_at = 20;
    after.last_heartbeat_at = Some(20);
    after.health = NodeHealth::Degraded;
    after.available = false;
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: Some(Box::new(SyncResourceSnapshot::Computer(before.clone()))),
            after: Box::new(SyncResourceSnapshot::Computer(after)),
        }],
    )
    .await;
    assert!(fixture
        .sync
        .pending_resources(&fixture.binding, 32)
        .await
        .unwrap()
        .is_empty());
    fixture
        .sync
        .set_enabled(&fixture.binding.scope.account_id, false)
        .await
        .unwrap();
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Computer(before)),
        }],
    )
    .await;
    assert_eq!(
        fixture
            .sync
            .pending_resources(&fixture.binding, 32)
            .await
            .unwrap(),
        vec![resource]
    );
}

#[tokio::test]
async fn transcript_import_cursor_and_event_mapping_share_the_business_commit() {
    let source = fixture().await;
    let target = fixture().await;
    let (session, events) = transcript::session_and_events(COMPUTER);
    let resource = SyncResourceKey::Session {
        session_id: session.session.id.clone(),
    };
    let mut changes = vec![
        SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Session(Box::new(session))),
        },
        SyncChange::Turn(SyncTurnMetadata {
            identity: events[0].identity().clone(),
            coordinator_id: source.binding.peer_id.clone(),
            previous_turn_id: None,
        }),
    ];
    changes.extend(events.iter().enumerate().map(|(index, event)| {
        SyncChange::Event(SyncTimelineEntry {
            key: SyncEventKey::Agent {
                identity: event.identity().clone(),
                sequence: index as u64 + 1,
            },
            event: event.clone(),
            persisted_at: 100 + index as i64,
        })
    }));
    capture(&source, resource.clone(), changes).await;
    let batch = source
        .sync
        .export_pending(&source.binding, resource.clone(), 32)
        .await
        .unwrap()
        .unwrap();
    let mut tx = target.storage.begin_transaction().await.unwrap();
    target
        .sync
        .import_document(
            &mut *tx,
            &target.binding,
            &resource,
            std::slice::from_ref(&batch.update),
            &batch.version,
            Some("feed-7"),
        )
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert!(target
        .sync
        .document_version(&target.binding, &resource)
        .await
        .unwrap()
        .is_none());
    assert!(target
        .sync
        .download_cursor(&target.binding)
        .await
        .unwrap()
        .is_none());
    let mut tx = target.storage.begin_transaction().await.unwrap();
    let (version, imported) = target
        .sync
        .import_document(
            &mut *tx,
            &target.binding,
            &resource,
            &[batch.update],
            &batch.version,
            Some("feed-7"),
        )
        .await
        .unwrap();
    for (index, expected) in events.iter().enumerate() {
        let entry = imported
            .iter()
            .find_map(|change| match change {
                talos_sync_types::SyncChange::Event(entry)
                    if entry.key
                        == talos_sync_types::SyncEventKey::Agent {
                            identity: projection::transcode(expected.identity()).unwrap(),
                            sequence: index as u64 + 1,
                        } =>
                {
                    Some(entry)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            projection::transcode::<_, talos_agent_api::TimelineEventPayload>(&entry.event)
                .unwrap(),
            *expected
        );
        target
            .sync
            .map_event(
                &mut *tx,
                SyncEventMapping {
                    scope: target.binding.scope.clone(),
                    session_id: resource.id().into(),
                    generation: 0,
                    key: SyncEventKey::Agent {
                        identity: expected.identity().clone(),
                        sequence: index as u64 + 1,
                    },
                    local_event_id: format!("target-{}", 800 + index),
                    local_event_seq: 800 + index as i64,
                },
            )
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(
        target
            .sync
            .document_version(&target.binding, &resource)
            .await
            .unwrap(),
        Some(version)
    );
    assert_eq!(
        target.sync.download_cursor(&target.binding).await.unwrap(),
        Some("feed-7".into())
    );
    assert!(target
        .sync
        .pending_resources(&target.binding, 32)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn account_rebind_and_computer_identity_change_fail_without_reassigning_data() {
    let fixture = fixture().await;
    let actor = RequestIdentity {
        user_id: "account-2".into(),
        username: None,
        role: UserRole::User,
        auth_disabled: false,
        access_token: None,
    };
    let error = fixture
        .sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: SyncScope {
                    account_id: actor.user_id.clone(),
                    ..fixture.binding.scope.clone()
                },
                source_user_id: fixture.binding.source_user_id.clone(),
                computers: fixture.binding.computers.clone(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::Conflict);
    let actor = RequestIdentity {
        user_id: fixture.binding.scope.account_id.clone(),
        username: None,
        role: UserRole::User,
        auth_disabled: false,
        access_token: None,
    };
    let error = fixture
        .sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: fixture.binding.scope.clone(),
                source_user_id: fixture.binding.source_user_id.clone(),
                computers: vec![ComputerAlias {
                    source_node_id: "embedded".into(),
                    computer_id: uuid::Uuid::new_v4().to_string(),
                }],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::Conflict);
    assert_eq!(
        fixture
            .sync
            .binding(&fixture.binding.scope, &fixture.binding.source_user_id)
            .await
            .unwrap(),
        Some(fixture.binding)
    );
}

#[tokio::test]
async fn account_opt_out_precedes_binding_and_survives_the_schema_upgrade() {
    let home = tempfile::tempdir().unwrap();
    let migration_dir = home.path().join("migrations");
    std::fs::create_dir(&migration_dir).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let upgrade = "202610130001_sync_account_preferences.sql";
    for entry in std::fs::read_dir(&source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() != upgrade {
            std::fs::copy(entry.path(), migration_dir.join(entry.file_name())).unwrap();
        }
    }
    let storage = Arc::new(
        SqliteStorage::new(home.path().join("talos.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(storage.pool(), &migration_dir)
        .await
        .unwrap();
    storage.execute("INSERT INTO sync_bindings(hub_id, account_id, source_user_id, enabled, computers_json, download_cursor) VALUES ('hub-1', 'saved-account', 'saved-account', 0, '[]', 'saved-cursor')", &[]).await.unwrap();
    std::fs::copy(source.join(upgrade), migration_dir.join(upgrade)).unwrap();
    talos_store::migration::run_migrations(storage.pool(), &migration_dir)
        .await
        .unwrap();
    let sync = TalosSync::open(storage.clone(), options()).await.unwrap();
    assert!(!sync.enabled("saved-account").await.unwrap());
    assert!(sync.enabled("new-account").await.unwrap());
    sync.set_enabled("new-account", false).await.unwrap();
    assert!(sync
        .binding(
            &SyncScope {
                hub_id: "hub-1".into(),
                account_id: "new-account".into()
            },
            "new-account"
        )
        .await
        .unwrap()
        .is_none());
    drop(sync);
    let sync = TalosSync::open(storage, options()).await.unwrap();
    assert!(!sync.enabled("new-account").await.unwrap());
    assert!(sync.enabled("other-account").await.unwrap());
    let actor = RequestIdentity {
        user_id: "new-account".into(),
        username: None,
        role: UserRole::User,
        auth_disabled: false,
        access_token: None,
    };
    let binding = sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: SyncScope {
                    hub_id: "hub-1".into(),
                    account_id: actor.user_id.clone(),
                },
                source_user_id: actor.user_id.clone(),
                computers: vec![],
            },
        )
        .await
        .unwrap();
    assert!(!sync.enabled(&actor.user_id).await.unwrap());
    sync.set_enabled(&actor.user_id, true).await.unwrap();
    assert!(sync.enabled(&actor.user_id).await.unwrap());
    assert!(sync.download_cursor(&binding).await.unwrap().is_none());
    let saved = sync
        .binding(
            &SyncScope {
                hub_id: "hub-1".into(),
                account_id: "saved-account".into(),
            },
            "saved-account",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        sync.download_cursor(&saved).await.unwrap().as_deref(),
        Some("saved-cursor")
    );
}
