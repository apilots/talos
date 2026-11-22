use super::*;

#[tokio::test]
async fn replicated_computer_tombstone_preserves_server_directory_and_does_not_block_history() {
    let source = peer(true).await;
    let target = peer(false).await;
    registered_computer(&source).await;
    registered_computer(&target).await;
    source
        .sessions
        .insert_session_record(&record("retained-history"))
        .await
        .unwrap();
    run(&source, "retained-history", "turn-1").await;
    let nodes = DatabaseAgentNodeStore::new(&source.storage);
    nodes
        .delete_owned(
            COMPUTER,
            "account-1",
            /*expected_lease_token_hash*/ None,
            20,
        )
        .await
        .unwrap();
    exchange(
        &source,
        &target,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    assert!(DatabaseAgentNodeStore::new(&target.storage)
        .get(COMPUTER)
        .await
        .unwrap()
        .is_some());
    let resource = SyncResourceKey::Session {
        session_id: "retained-history".into(),
    };
    collect(&source.adapter, &resource).await;
    let exported = source
        .sync
        .export_pending(&source.adapter.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let (mut tx, batch) = target
        .sync
        .prepare_import(
            target.adapter.as_ref(),
            &target.adapter.binding,
            &resource,
            &[exported.update],
            &exported.version,
        )
        .await
        .unwrap();
    assert!(target
        .adapter
        .missing_dependencies(&mut *tx, &batch)
        .await
        .unwrap()
        .is_empty());
    target.adapter.apply(&mut *tx, &batch).await.unwrap();
    tx.commit().await.unwrap();
    assert!(target
        .sessions
        .get_session_record("retained-history")
        .await
        .unwrap()
        .is_some());
    assert!(target
        .sessions
        .get_agent_active_turn("retained-history")
        .await
        .unwrap()
        .is_none());
    let deletions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM computer_deletions WHERE node_id=?")
            .bind(COMPUTER)
            .fetch_one(target.storage.pool())
            .await
            .unwrap();
    assert_eq!(deletions, 0);
}

#[tokio::test]
async fn local_registration_retirement_freezes_history_and_new_enrollment_uses_a_fresh_alias() {
    let source = peer(true).await;
    registered_computer(&source).await;
    source
        .sessions
        .insert_session_record(&record("old-local-history"))
        .await
        .unwrap();
    run(&source, "old-local-history", "turn-1").await;
    source
        .sessions
        .record_local_registration_removal("account-1", COMPUTER)
        .await
        .unwrap();
    source
        .sessions
        .insert_session_record(&record("created-after-removal-before-reconnect"))
        .await
        .unwrap();
    assert!(source
        .sync
        .export_pending(
            &source.adapter.binding,
            SyncResourceKey::Session {
                session_id: "created-after-removal-before-reconnect".into()
            },
            128
        )
        .await
        .unwrap()
        .is_none());
    source
        .sessions
        .retire_local_registration("account-1", COMPUTER)
        .await
        .unwrap();
    assert_eq!(
        source
            .sessions
            .get_session_record("old-local-history")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        COMPUTER
    );
    assert_eq!(
        source
            .sessions
            .get_session_record("created-after-removal-before-reconnect")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        "embedded"
    );
    let mut next = source
        .sync
        .binding(&source.adapter.binding.scope, "account-1")
        .await
        .unwrap()
        .unwrap();
    assert!(next.computers.is_empty());
    let retired = Arc::new(StoreSyncAdapter::new(
        source.storage.clone(),
        source.sync.clone(),
        next.clone(),
        Arc::new(projection),
    ));
    source.sessions.install_sync(retired).unwrap();
    source
        .sessions
        .insert_session_record(&record("unregistered-local"))
        .await
        .unwrap();
    // Repeating recovery cannot reassign newly created local records to the deleted UUID.
    source
        .sessions
        .retire_local_registration("account-1", COMPUTER)
        .await
        .unwrap();
    assert_eq!(
        source
            .sessions
            .get_session_record("unregistered-local")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        "embedded"
    );
    let actor = RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    };
    let fresh = uuid::Uuid::new_v4().to_string();
    next = source
        .sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: next.scope,
                source_user_id: next.source_user_id,
                computers: vec![ComputerAlias {
                    source_node_id: "embedded".into(),
                    computer_id: fresh.clone(),
                }],
            },
        )
        .await
        .unwrap();
    let adapter = Arc::new(StoreSyncAdapter::new(
        source.storage.clone(),
        source.sync.clone(),
        next.clone(),
        Arc::new(projection),
    ));
    source.sessions.install_sync(adapter.clone()).unwrap();
    source
        .sessions
        .insert_session_record(&record("fresh-local"))
        .await
        .unwrap();
    collect(
        &adapter,
        &SyncResourceKey::Session {
            session_id: "old-local-history".into(),
        },
    )
    .await;
    collect(
        &adapter,
        &SyncResourceKey::Session {
            session_id: "fresh-local".into(),
        },
    )
    .await;
    let old = source
        .sync
        .export_pending(
            &next,
            SyncResourceKey::Session {
                session_id: "old-local-history".into(),
            },
            128,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(old.through_sequence > 0);
    let mut tx = source.storage.begin_transaction().await.unwrap();
    let old = source
        .sync
        .session_snapshot(&mut *tx, &next, "old-local-history")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.binding.computer_id, COMPUTER);
    tx.commit().await.unwrap();
    let changes = source
        .sync
        .export_pending(
            &next,
            SyncResourceKey::Session {
                session_id: "fresh-local".into(),
            },
            128,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(changes.through_sequence > 0);
    let mut tx = source.storage.begin_transaction().await.unwrap();
    let new = source
        .sync
        .session_snapshot(&mut *tx, &next, "fresh-local")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(new.binding.computer_id, fresh);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn replicated_deletion_does_not_retire_capture_before_server_confirmation() {
    let source = peer(false).await;
    let target = peer(true).await;
    registered_computer(&source).await;
    target
        .sessions
        .insert_session_record(&record("pre-import-history"))
        .await
        .unwrap();
    run(&target, "pre-import-history", "turn-1").await;
    DatabaseAgentNodeStore::new(&source.storage)
        .delete_owned(
            COMPUTER,
            "account-1",
            /*expected_lease_token_hash*/ None,
            20,
        )
        .await
        .unwrap();
    exchange(
        &source,
        &target,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    target
        .sessions
        .insert_session_record(&record("post-import-local"))
        .await
        .unwrap();
    collect(
        &target.adapter,
        &SyncResourceKey::Session {
            session_id: "post-import-local".into(),
        },
    )
    .await;
    assert!(target
        .sync
        .export_pending(
            &target.adapter.binding,
            SyncResourceKey::Session {
                session_id: "post-import-local".into()
            },
            128
        )
        .await
        .unwrap()
        .is_some());
    target
        .sessions
        .retire_local_registration("account-1", COMPUTER)
        .await
        .unwrap();
    assert_eq!(
        target
            .sessions
            .get_session_record("pre-import-history")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        COMPUTER
    );
    assert_eq!(
        target
            .sessions
            .get_session_record("post-import-local")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        COMPUTER
    );
    assert!(target
        .sessions
        .local_registration_retired("account-1", COMPUTER)
        .await
        .unwrap());
}
