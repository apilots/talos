use super::*;

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
        auth_disabled: false,
        access_token: None,
    };
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
