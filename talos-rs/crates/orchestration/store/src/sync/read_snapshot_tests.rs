use super::*;

#[tokio::test]
async fn settled_turn_display_does_not_restore_the_sync_document() {
    let source = peer(true).await;
    let replica = peer(false).await;
    registered_computer(&source).await;
    exchange(
        &source,
        &replica,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    let id = "settled-with-unreadable-sync";
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    run(&source, id, "completed-turn").await;
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    let scan = || SyncScanParams {
        binding: source.adapter.binding.clone(),
        cursor: None,
        limit: 32,
    };
    assert!(source
        .adapter
        .scan(scan())
        .await
        .unwrap()
        .resources
        .contains(&resource));
    exchange(&source, &replica, resource.clone()).await;
    assert!(!source
        .adapter
        .scan(scan())
        .await
        .unwrap()
        .resources
        .contains(&resource));
    assert_eq!(
        replica.storage.execute(
            "UPDATE sync_documents SET snapshot_blob = x'00' WHERE kind = 'session' AND resource_id = ?",
            &[id],
        ).await.unwrap(),
        1,
    );
    assert!(replica
        .sessions
        .get_shared_active_turn(id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn retained_session_reads_do_not_wait_for_a_sync_writer() {
    let source = peer(true).await;
    let replica = peer(false).await;
    registered_computer(&source).await;
    exchange(
        &source,
        &replica,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    let id = "retained-during-import";
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    run(&source, id, "completed-turn").await;
    exchange(
        &source,
        &replica,
        SyncResourceKey::Session {
            session_id: id.into(),
        },
    )
    .await;
    let actor = RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    };
    replica.storage.execute(
        "UPDATE sync_documents SET session_projection_json=NULL WHERE kind='session' AND resource_id=?",
        &[id],
    ).await.unwrap();
    assert!(replica
        .sessions
        .get_shared_session_info(id)
        .await
        .unwrap()
        .is_some());
    replica.adapter.start_capture();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let ready = replica.storage.query(
                "SELECT 1 FROM sync_documents WHERE kind='session' AND resource_id=? AND session_projection_json IS NOT NULL",
                &[id],
            ).await.unwrap();
            if !ready.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("Retained documents should get a projection without a Hub connection");
    for peer in [&source, &replica] {
        let expected = peer
            .sessions
            .get_shared_session_info(id)
            .await
            .unwrap()
            .unwrap();
        let cursor = peer.sessions.get_history_cursor(id).await.unwrap().unwrap();
        let active = peer.sessions.get_shared_active_turn(id).await.unwrap();
        assert_eq!(cursor.completed_turn_id.as_deref(), Some("completed-turn"));
        peer.storage.execute(
            "UPDATE sync_documents SET snapshot_blob=x'01' WHERE kind='session' AND resource_id=?",
            &[id],
        ).await.unwrap();
        let mut writer = peer.storage.begin_transaction().await.unwrap();
        writer
            .execute(
                "UPDATE sessions SET title = 'uncommitted import' WHERE session_key = ?",
                &[id],
            )
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            assert_eq!(
                peer.sessions.get_shared_session_info(id).await.unwrap(),
                Some(expected)
            );
            assert_eq!(
                peer.sessions.get_history_cursor(id).await.unwrap(),
                Some(cursor)
            );
            assert!(peer.sync.enabled(&actor.user_id).await.unwrap());
            assert_eq!(
                peer.sessions.get_shared_active_turn(id).await.unwrap(),
                active
            );
            assert_eq!(
                peer.sync.account_binding(&actor).await.unwrap(),
                Some(peer.adapter.binding.clone())
            );
        })
        .await
        .expect(
            "committed local metadata, History cut and status must not queue behind Sync writes",
        );
        writer.rollback().await.unwrap();
    }
}
