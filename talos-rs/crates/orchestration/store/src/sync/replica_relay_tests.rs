use super::*;
use talos_sync_sdk::{SyncChange as WireChange, SyncReplica};

async fn checkpoint(peer: &Peer, id: &str) -> Vec<u8> {
    let (inline, chunks): (Vec<u8>, Option<String>) = sqlx::query_as(
        "SELECT snapshot_blob, snapshot_id FROM sync_documents
         WHERE hub_id=? AND account_id=? AND kind='session' AND resource_id=?",
    )
    .bind(&peer.adapter.binding.scope.hub_id)
    .bind(&peer.adapter.binding.scope.account_id)
    .bind(id)
    .fetch_one(peer.storage.pool())
    .await
    .unwrap();
    if let Some(chunks) = chunks {
        let parts: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT contents FROM sync_snapshot_chunks WHERE snapshot_id=? ORDER BY ordinal",
        )
        .bind(chunks)
        .fetch_all(peer.storage.pool())
        .await
        .unwrap();
        parts.concat()
    } else {
        inline
    }
}

async fn publish(peer: &Peer, key: &SyncResourceKey, hub: &SyncReplica) {
    let version = hub.version().unwrap();
    let (mut tx, batch) = peer
        .sync
        .prepare_import(
            peer.adapter.as_ref(),
            peer.adapter.binding(),
            key,
            &[hub.snapshot().unwrap()],
            &version,
        )
        .await
        .unwrap();
    peer.adapter.apply(&mut *tx, &batch).await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn surviving_replica_restores_all_native_turns_without_rewriting_node_or_origins() {
    let a = peer(true).await;
    let b = peer(false).await;
    let c = peer(false).await;
    registered_computer(&a).await;
    let computer = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    exchange(&a, &b, computer.clone()).await;
    exchange(&a, &c, computer).await;
    let id = "offline-origin-history";
    let key = SyncResourceKey::Session {
        session_id: id.into(),
    };
    a.sessions.insert_session_record(&record(id)).await.unwrap();
    run(&a, id, "turn-1").await;
    run(&a, id, "turn-2").await;
    exchange(&a, &b, key.clone()).await;
    run(&b, id, "turn-3").await;
    run(&b, id, "turn-4").await;
    collect(&b.adapter, &key).await;
    let pending = b
        .sync
        .export_pending(b.adapter.binding(), key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    b.sync.acknowledge(&pending).await.unwrap();
    let expected = b
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    let wire_key = pending.resource.wire();
    let saved = checkpoint(&b, id).await;
    let original = SyncReplica::from_snapshot(wire_key.clone(), &saved).unwrap();
    let origins: Vec<_> = original
        .changes()
        .unwrap()
        .into_iter()
        .filter(|change| matches!(change, WireChange::Stream(_) | WireChange::Record(_)))
        .collect();
    assert_eq!(
        origins
            .iter()
            .filter(|change| matches!(change, WireChange::Stream(_)))
            .count(),
        4
    );
    let node_header = original.header().unwrap();
    assert_eq!(
        node_header.as_ref().unwrap().related.as_ref().unwrap().id(),
        COMPUTER
    );
    drop(a);

    // The same SDK import used by the authenticated Hub accepts B's retained checkpoint.
    let mut hub = SyncReplica::new(wire_key, 900).unwrap();
    hub.import_from_account(&[saved], "account-1").unwrap();
    assert_eq!(hub.header().unwrap(), node_header);
    assert_eq!(hub.changes().unwrap(), original.changes().unwrap());
    publish(&c, &key, &hub).await;
    publish(&c, &key, &hub).await;
    let actual = c
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    assert_eq!(
        actual.iter().map(|e| &e.event).collect::<Vec<_>>(),
        expected.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    let restored = c.sessions.get_session_record(id).await.unwrap().unwrap();
    assert_eq!(restored.execution_node_id, COMPUTER);
    assert_eq!(restored.source_type, record(id).source_type);
    assert_eq!(restored.source_provider, record(id).source_provider);
    assert!(c
        .sessions
        .get_agent_active_turn(id)
        .await
        .unwrap()
        .is_none());

    // B's subsequent Turn is incremental and must preserve the earlier publishers.
    run(&b, id, "turn-5").await;
    collect(&b.adapter, &key).await;
    let delta = b
        .sync
        .export_pending(b.adapter.binding(), key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    hub.import_from_account(&[delta.update], "account-1")
        .unwrap();
    assert_eq!(hub.header().unwrap(), node_header);
    publish(&c, &key, &hub).await;
    let latest = b
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    let restored = c
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    assert_eq!(
        restored.iter().map(|e| &e.event).collect::<Vec<_>>(),
        latest.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert_eq!(
        hub.changes()
            .unwrap()
            .into_iter()
            .filter(|change| origins.contains(change))
            .count(),
        origins.len()
    );
}

#[tokio::test]
async fn new_hub_binding_preserves_origins_pending_writes_and_original_replica_rejoin() {
    let a = peer(true).await;
    let b = peer(false).await;
    let mut c = peer(false).await;
    registered_computer(&a).await;
    let computer = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    exchange(&a, &b, computer.clone()).await;
    exchange(&a, &c, computer).await;
    let id = "fresh-hub-native-history";
    let key = SyncResourceKey::Session {
        session_id: id.into(),
    };
    a.sessions.insert_session_record(&record(id)).await.unwrap();
    run(&a, id, "turn-a-1").await;
    run(&a, id, "turn-a-2").await;
    exchange(&a, &b, key.clone()).await;
    let original = checkpoint(&a, id).await;
    let original_version = SyncReplica::from_snapshot(key.wire(), &original)
        .unwrap()
        .version()
        .unwrap();
    run(&b, id, "turn-b-3").await;
    run(&b, id, "turn-b-4").await;
    collect(&b.adapter, &key).await;
    let captured = b
        .sync
        .export_pending(b.adapter.binding(), key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    b.sync.acknowledge(&captured).await.unwrap();
    run(&b, id, "turn-b-5-pending").await;
    collect(&b.adapter, &key).await;
    let expected = b
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    a.sessions
        .append_timeline_notice(TimelineSystemNotice {
            identity: AgentTurnRef::new(id, "turn-a-2"),
            occurred_at: 12,
            notice: AgentSystemNotice {
                level: TalosTimelineNoticeLevel::Info,
                content: SystemNoticeContent::Text("Original A's subsequent notice".into()),
            },
        })
        .await
        .unwrap();
    collect(&a.adapter, &key).await;
    let late = a
        .sync
        .export_pending(a.adapter.binding(), key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let source_a = a.adapter.binding.peer_id.clone();
    let source_b = b.adapter.binding.peer_id.clone();
    drop(a);
    let actor = RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    };
    let binding = b
        .sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: SyncScope {
                    hub_id: "replacement-hub".into(),
                    account_id: actor.user_id.clone(),
                },
                source_user_id: actor.user_id.clone(),
                computers: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        b.sync.account_binding(&actor).await.unwrap(),
        Some(binding.clone())
    );
    let c_binding = c
        .sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: binding.scope.clone(),
                source_user_id: actor.user_id.clone(),
                computers: vec![],
            },
        )
        .await
        .unwrap();
    c.adapter = Arc::new(StoreSyncAdapter::new(
        c.storage.clone(),
        c.sync.clone(),
        c_binding,
        Arc::new(projection),
    ));

    let fresh = StoreSyncAdapter::new(
        b.storage.clone(),
        b.sync.clone(),
        binding.clone(),
        Arc::new(projection),
    );
    let page = fresh
        .scan(talos_sync::SyncScanParams {
            binding: binding.clone(),
            cursor: None,
            limit: 128,
        })
        .await
        .unwrap();
    assert!(
        !page.resources.contains(&key),
        "retained history must not be reconstructed as a new source"
    );
    let baseline = b
        .sync
        .export_pending(&binding, key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let mut hub = SyncReplica::new(key.wire(), 900).unwrap();
    hub.import_from_account(std::slice::from_ref(&baseline.update), "account-1")
        .unwrap();
    let mut origin_counts = std::collections::BTreeMap::new();
    for change in hub.changes().unwrap() {
        if let WireChange::Stream(stream) = change {
            *origin_counts.entry(stream.owner).or_insert(0) += 1;
        }
    }
    assert_eq!(
        origin_counts,
        std::collections::BTreeMap::from([(source_a, 2), (source_b, 3)])
    );
    publish(&c, &key, &hub).await;
    let actual = c
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    assert_eq!(
        actual.iter().map(|e| &e.event).collect::<Vec<_>>(),
        expected.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    // An original replica can return after B seeds the new Hub, preserving both sources.
    hub.import_from_account(&[original], "account-1").unwrap();
    assert!(talos_sync_sdk::version_includes(&hub.version().unwrap(), &original_version).unwrap());
    hub.import_from_account(&[late.update], "account-1")
        .unwrap();
    publish(&c, &key, &hub).await;
    let events = c
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    assert_eq!(events.len(), expected.len() + 1);
    assert_eq!(
        c.sessions
            .get_session_record(id)
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        COMPUTER
    );
    b.sync.acknowledge(&baseline).await.unwrap();
    assert!(b
        .sync
        .export_pending(&binding, key, 128)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn new_hub_binding_retains_deleted_documents_that_are_absent_from_native_lists() {
    let a = peer(true).await;
    let b = peer(false).await;
    registered_computer(&a).await;
    exchange(
        &a,
        &b,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    let id = "deleted-before-new-hub";
    let key = SyncResourceKey::Session {
        session_id: id.into(),
    };
    a.sessions.insert_session_record(&record(id)).await.unwrap();
    run(&a, id, "original-turn").await;
    exchange(&a, &b, key.clone()).await;
    a.sessions.delete_session_tree(id).await.unwrap();
    exchange(&a, &b, key.clone()).await;
    assert!(b.sessions.get_session_record(id).await.unwrap().is_none());
    drop(a);
    let actor = RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    };
    let binding = b
        .sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: SyncScope {
                    hub_id: "replacement-hub".into(),
                    account_id: actor.user_id.clone(),
                },
                source_user_id: actor.user_id.clone(),
                computers: vec![],
            },
        )
        .await
        .unwrap();
    let baseline = b
        .sync
        .export_pending(&binding, key.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let mut hub = SyncReplica::new(key.wire(), 900).unwrap();
    hub.import_from_account(&[baseline.update], "account-1")
        .unwrap();
    assert!(hub.summary().unwrap().deleted);
}
