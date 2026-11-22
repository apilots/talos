//! Real SQLite state containing snapshots produced by the published SDK 0.1.7.
use super::*;
use talos_sync_sdk::SyncReplica;
fn fixture(name: &str) -> (Vec<u8>, talos_sync::SyncVersion, Vec<SyncChange>) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sync-parser/testdata/released-v1");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(format!("{name}.json"))).unwrap()).unwrap();
    (
        std::fs::read(root.join(format!("{name}.bin"))).unwrap(),
        serde_json::from_value(value["version"].clone()).unwrap(),
        serde_json::from_value(value["changes"].clone()).unwrap(),
    )
}
async fn seed(
    peer: &Peer,
    name: &str,
    pending: bool,
) -> (SyncResourceKey, Vec<talos_agent_api::TimelineEventPayload>) {
    let (snapshot, version, mut changes) = fixture(name);
    let key = SyncResourceKey::Session {
        session_id: if name == "deleted" {
            "released-deleted"
        } else {
            "released-session"
        }
        .into(),
    };
    let pending_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sync-parser/testdata/released-v1/pending.json");
    let pending_changes: Vec<SyncChange> =
        serde_json::from_slice(&std::fs::read(pending_file).unwrap()).unwrap();
    if pending {
        changes.extend(pending_changes.clone());
    }
    let expected = changes
        .iter()
        .filter_map(|change| {
            if let SyncChange::Event(event) = change {
                Some(event.event.clone())
            } else {
                None
            }
        })
        .collect();
    let batch = SyncImportBatch {
        scope: peer.adapter.binding.scope.clone(),
        resource: key.clone(),
        version: version.clone(),
        changes,
    };
    peer.adapter.prepare_import(&batch).await.unwrap();
    let mut tx = peer.storage.begin_transaction().await.unwrap();
    peer.adapter.apply(&mut *tx, &batch).await.unwrap();
    tx.commit().await.unwrap();
    let deleted = matches!(batch.changes.last(), Some(SyncChange::Delete));
    sqlx::query("INSERT INTO sync_documents(hub_id,account_id,kind,resource_id,computer_id,generation,deleted,snapshot_blob,version_vector,acknowledged_vector,acknowledged_generation) VALUES ('hub-1','account-1','session',?,?,?,?,?,?,?,?)")
        .bind(key.id()).bind(COMPUTER).bind(version.history_generation as i64).bind(deleted).bind(snapshot)
        .bind(&version.version_vector).bind(&version.version_vector).bind(version.history_generation as i64).execute(peer.storage.pool()).await.unwrap();
    if pending {
        sqlx::query("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,created_at) VALUES ('hub-1','account-1','account-1','session','released-session',?,99)")
            .bind(serde_json::to_string(&pending_changes).unwrap()).execute(peer.storage.pool()).await.unwrap();
    }
    (key, expected)
}
async fn verify(pending: bool) {
    let b = peer(false).await;
    let mut c = peer(false).await;
    let (key, expected) = seed(&b, "survivor-b", pending).await;
    // Exercise the real schema upgrade from the released database layout.
    sqlx::query("ALTER TABLE sync_documents DROP COLUMN replica_schema_version")
        .execute(b.storage.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version=202611220001")
        .execute(b.storage.pool())
        .await
        .unwrap();
    crate::migration::run_migrations(b.storage.pool(), &crate::migration::test_migrations_dir())
        .await
        .unwrap();
    let actor = RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    };
    let params = SyncBindParams {
        scope: SyncScope {
            hub_id: "new-v2-hub".into(),
            account_id: actor.user_id.clone(),
        },
        source_user_id: actor.user_id.clone(),
        computers: vec![],
    };
    b.sync.upgrade_local_replicas().await.unwrap();
    let binding = b.sync.bind_account(&actor, params.clone()).await.unwrap();
    c.adapter = Arc::new(StoreSyncAdapter::new(
        c.storage.clone(),
        c.sync.clone(),
        c.sync.bind_account(&actor, params).await.unwrap(),
        Arc::new(projection),
    ));
    let mut hub = SyncReplica::new(key.wire(), 900).unwrap();
    while let Some(batch) = b
        .sync
        .export_pending(&binding, key.clone(), 128)
        .await
        .unwrap()
    {
        hub.import_from_account(std::slice::from_ref(&batch.update), "account-1")
            .unwrap();
        b.sync.acknowledge(&batch).await.unwrap();
    }
    let version = hub.version().unwrap();
    let (mut tx, batch) = c
        .sync
        .prepare_import(
            c.adapter.as_ref(),
            c.adapter.binding(),
            &key,
            &[hub.snapshot().unwrap()],
            &version,
        )
        .await
        .unwrap();
    c.adapter.apply(&mut *tx, &batch).await.unwrap();
    tx.commit().await.unwrap();
    let actual = c
        .sessions
        .list_timeline_events_after("released-session", 0, 100)
        .await
        .unwrap();
    let mut actual: Vec<_> = actual
        .into_iter()
        .map(|entry| serde_json::to_string(&entry.event).unwrap())
        .collect();
    actual.sort();
    let mut expected: Vec<_> = expected
        .iter()
        .map(|entry| serde_json::to_string(entry).unwrap())
        .collect();
    expected.sort();
    assert_eq!(actual, expected);
    let record = c
        .sessions
        .get_session_record("released-session")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.execution_node_id, COMPUTER);
    assert_eq!(
        record.title,
        if pending {
            "Offline pending title"
        } else {
            "Released history"
        }
    );
    assert_eq!(
        hub.changes()
            .unwrap()
            .iter()
            .filter(|change| matches!(change, talos_sync_sdk::SyncChange::Stream(_)))
            .count(),
        4
    );
    // Continue with a normal native committed notice and deliver only its delta.
    let current = Arc::new(StoreSyncAdapter::new(
        b.storage.clone(),
        b.sync.clone(),
        binding.clone(),
        Arc::new(projection),
    ));
    b.sessions.install_sync(current.clone()).unwrap();
    b.sessions
        .append_timeline_notice(TimelineSystemNotice {
            identity: AgentTurnRef::new("released-session", "turn-4"),
            occurred_at: 123,
            notice: AgentSystemNotice {
                level: TalosTimelineNoticeLevel::Info,
                content: SystemNoticeContent::Text("After upgrade".into()),
            },
        })
        .await
        .unwrap();
    collect(&current, &key).await;
    let delta = b
        .sync
        .export_pending(&binding, key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    hub.import_from_account(std::slice::from_ref(&delta.update), "account-1")
        .unwrap();
    let (mut tx, batch) = c
        .sync
        .prepare_import(
            c.adapter.as_ref(),
            c.adapter.binding(),
            &key,
            std::slice::from_ref(&delta.update),
            &hub.version().unwrap(),
        )
        .await
        .unwrap();
    c.adapter.apply(&mut *tx, &batch).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        c.sessions
            .list_timeline_events_after("released-session", 0, 100)
            .await
            .unwrap()
            .len(),
        expected.len() + 1
    );
}
#[tokio::test]
async fn released_database_with_empty_outbox_restores_into_v2_and_continues_incrementally() {
    verify(false).await;
}
#[tokio::test]
async fn released_database_preserves_unexported_native_changes_during_upgrade() {
    verify(true).await;
}

async fn exported(
    peer: &Peer,
    key: &SyncResourceKey,
    changes: &[SyncChange],
    version: &talos_sync::SyncVersion,
) -> i64 {
    let proof = serde_json::to_string(version).unwrap();
    let seq:i64=sqlx::query_scalar("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,exported_version_json,created_at) VALUES ('hub-1','account-1','account-1','session',?,?,?,99) RETURNING seq")
        .bind(key.id()).bind(serde_json::to_string(changes).unwrap()).bind(&proof).fetch_one(peer.storage.pool()).await.unwrap();
    sqlx::query("INSERT INTO sync_resource_capture(hub_id,account_id,kind,resource_id,revision,outbox_sequence,exported_version_json) VALUES ('hub-1','account-1','session',?,?,?,?)")
        .bind(key.id()).bind(seq).bind(seq).bind(proof).execute(peer.storage.pool()).await.unwrap();
    seq
}
fn actor() -> RequestIdentity {
    RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    }
}
async fn verify_materialized_metadata(with_tail: bool) {
    let b = peer(false).await;
    let (key, _) = seed(&b, "materialized-b", false).await;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sync-parser/testdata/released-v1/materialized-b.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(root).unwrap()).unwrap();
    let changes: Vec<SyncChange> =
        serde_json::from_value(value["exported"]["changes"].clone()).unwrap();
    let version = serde_json::from_value(value["exported"]["version"].clone()).unwrap();
    let seq = exported(&b, &key, &changes, &version).await;
    if with_tail {
        let pending_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sync-parser/testdata/released-v1/pending.json");
        let pending: Vec<SyncChange> =
            serde_json::from_slice(&std::fs::read(pending_path).unwrap()).unwrap();
        let (_, version, _) = fixture("materialized-b");
        let batch = SyncImportBatch {
            scope: b.adapter.binding.scope.clone(),
            resource: key.clone(),
            version,
            changes: pending.clone(),
        };
        b.adapter.prepare_import(&batch).await.unwrap();
        let mut tx = b.storage.begin_transaction().await.unwrap();
        b.adapter.apply(&mut *tx, &batch).await.unwrap();
        tx.commit().await.unwrap();
        let tail:i64=sqlx::query_scalar("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,created_at) VALUES ('hub-1','account-1','account-1','session',?,?,100) RETURNING seq")
            .bind(key.id()).bind(serde_json::to_string(&pending).unwrap()).fetch_one(b.storage.pool()).await.unwrap();
        sqlx::query("INSERT INTO sync_resource_capture(hub_id,account_id,kind,resource_id,revision,outbox_sequence) VALUES ('hub-1','account-1','session',?,?,?)")
            .bind(key.id()).bind(tail).bind(tail).execute(b.storage.pool()).await.unwrap();
    }
    b.sync.upgrade_local_replicas().await.unwrap();
    let old = b.adapter.binding();
    let mut hub = SyncReplica::from_snapshot(
        key.wire(),
        &super::replica_relay_tests::checkpoint(&b, key.id()).await,
    )
    .unwrap();
    let binding = b
        .sync
        .bind_account(
            &actor(),
            SyncBindParams {
                scope: old.scope.clone(),
                source_user_id: old.source_user_id.clone(),
                computers: old.computers.clone(),
            },
        )
        .await
        .unwrap();
    let proof: String =
        sqlx::query_scalar("SELECT exported_version_json FROM sync_outbox WHERE seq=?")
            .bind(seq)
            .fetch_one(b.storage.pool())
            .await
            .unwrap();
    let proof: talos_sync::SyncVersion = serde_json::from_str(&proof).unwrap();
    let upload = b
        .sync
        .export_pending(&binding, key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        talos_sync_sdk::version_equal(&proof, &upload.version).unwrap(),
        !with_tail,
        "only the genuine unapplied tail may add operations"
    );
    hub.import_from_account(&[upload.update], "account-1")
        .unwrap();
    let title = hub
        .changes()
        .unwrap()
        .into_iter()
        .find_map(|change| {
            let talos_sync_sdk::SyncChange::Fields { edits, .. } = change else {
                return None;
            };
            edits.into_iter().find_map(|edit| match edit {
                talos_sync_sdk::SyncFieldEdit::Put { key, value }
                    if key == "/data/session/title" =>
                {
                    Some(value)
                }
                _ => None,
            })
        })
        .unwrap();
    assert_eq!(
        title,
        serde_json::json!(if with_tail {
            "Offline pending title"
        } else {
            "Newer merged remote title"
        })
    );
}
#[tokio::test]
async fn materialized_v1_metadata_is_not_replayed_over_a_newer_remote_merge() {
    verify_materialized_metadata(false).await;
}
#[tokio::test]
async fn materialized_v1_prefix_and_unapplied_tail_remain_distinct() {
    verify_materialized_metadata(true).await;
}
#[tokio::test]
async fn materialized_v1_reset_and_delete_are_uploaded_without_reapplying_them() {
    let b = peer(false).await;
    let (key, _) = seed(&b, "deleted", false).await;
    let (_, version, _) = fixture("deleted");
    exported(
        &b,
        &key,
        &[SyncChange::Reset {
            history_generation: 1,
        }],
        &version,
    )
    .await;
    exported(&b, &key, &[SyncChange::Delete], &version).await;
    b.sync.upgrade_local_replicas().await.unwrap();
    let old = b.adapter.binding();
    let mut hub = SyncReplica::from_snapshot(
        key.wire(),
        &super::replica_relay_tests::checkpoint(&b, key.id()).await,
    )
    .unwrap();
    let binding = b
        .sync
        .bind_account(
            &actor(),
            SyncBindParams {
                scope: old.scope.clone(),
                source_user_id: old.source_user_id.clone(),
                computers: old.computers.clone(),
            },
        )
        .await
        .unwrap();
    let upload = b
        .sync
        .export_pending(&binding, key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    hub.import_from_account(&[upload.update], "account-1")
        .unwrap();
    assert_eq!(hub.version().unwrap().history_generation, 1);
    assert!(hub.summary().unwrap().deleted);
}
