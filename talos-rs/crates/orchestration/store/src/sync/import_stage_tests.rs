use super::*;

async fn prepare(
    peer: &Peer,
    upload: &talos_sync::SyncExportBatch,
) -> TalosResult<SyncImportBatch> {
    let (tx, batch) = peer
        .sync
        .prepare_import(
            peer.adapter.as_ref(),
            peer.adapter.binding(),
            &upload.resource,
            std::slice::from_ref(&upload.update),
            &upload.version,
        )
        .await?;
    tx.rollback().await?;
    Ok(batch)
}

async fn notice(store: &DatabaseSessionStore, id: &str, text: String) {
    store
        .append_timeline_notice(TimelineSystemNotice {
            identity: AgentTurnRef::new(id, "remote-turn"),
            occurred_at: 20,
            notice: AgentSystemNotice {
                level: TalosTimelineNoticeLevel::Info,
                content: SystemNoticeContent::Text(text),
            },
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_tombstone_releases_unpublished_history_for_a_cold_session() {
    let source = peer(true).await;
    let target = peer(false).await;
    registered_computer(&source).await;
    exchange(
        &source,
        &target,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    let id = "deleted-before-publication";
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    run(&source, id, "remote-turn").await;
    collect(&source.adapter, &resource).await;
    let live = source
        .sync
        .export_pending(source.adapter.binding(), resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    prepare(&target, &live).await.unwrap();
    let staged: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE session_id=? AND import_id<>0")
            .bind(id)
            .fetch_one(target.storage.pool())
            .await
            .unwrap();
    assert!(staged > 0);
    assert!(target
        .sessions
        .get_session_record(id)
        .await
        .unwrap()
        .is_none());

    source.sessions.delete_session_tree(id).await.unwrap();
    collect(&source.adapter, &resource).await;
    let deleted = source
        .sync
        .export_pending(source.adapter.binding(), resource, 128)
        .await
        .unwrap()
        .unwrap();
    let tombstone = prepare(&target, &deleted).await.unwrap();
    assert!(tombstone
        .changes
        .iter()
        .any(|change| matches!(change, SyncChange::Delete)));
    let remaining: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM agent_events WHERE session_id=?1),
                (SELECT COUNT(*) FROM sync_import_stages WHERE session_id=?1)",
    )
    .bind(id)
    .fetch_one(target.storage.pool())
    .await
    .unwrap();
    assert_eq!(remaining, (0, 0));
}

#[tokio::test]
async fn interrupted_import_resumes_hidden_batches_and_checks_the_session_tail() {
    let source = peer(true).await;
    let target = peer(false).await;
    registered_computer(&source).await;
    exchange(
        &source,
        &target,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    let id = "staged-history";
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    run(&source, id, "remote-turn").await;
    for index in 0..130 {
        notice(&source.sessions, id, format!("remote notice {index}")).await;
    }
    collect(&source.adapter, &resource).await;
    let upload = source
        .sync
        .export_pending(source.adapter.binding(), resource.clone(), 1024)
        .await
        .unwrap()
        .unwrap();
    target.storage.execute(
        "CREATE TRIGGER fail_second_stage_batch BEFORE INSERT ON agent_events
         WHEN NEW.import_id<>0 AND (SELECT COUNT(*) FROM agent_events WHERE import_id=NEW.import_id)>=128
         BEGIN SELECT RAISE(ABORT, 'test interruption'); END",
        &[],
    ).await.unwrap();
    assert!(prepare(&target, &upload).await.is_err());
    let first_stage: (i64, i64, i64) = sqlx::query_as(
        "SELECT id, next_event, complete FROM sync_import_stages WHERE session_id=?",
    )
    .bind(id)
    .fetch_one(target.storage.pool())
    .await
    .unwrap();
    assert_eq!((first_stage.1, first_stage.2), (128, 0));
    let visible: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM visible_agent_events WHERE session_id=?")
            .bind(id)
            .fetch_one(target.storage.pool())
            .await
            .unwrap();
    assert_eq!(visible, 0);
    assert!(target
        .sessions
        .get_session_record(id)
        .await
        .unwrap()
        .is_none());

    target
        .storage
        .execute("DROP TRIGGER fail_second_stage_batch", &[])
        .await
        .unwrap();
    let reopened_storage = Arc::new(
        SqliteStorage::new(target._home.path().join("facts.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    let reopened = Arc::new(
        TalosSync::open(
            reopened_storage.clone(),
            SyncOptions {
                hub_endpoint: "wss://sync.example.test/v1/sync".into(),
            },
        )
        .await
        .unwrap(),
    );
    let adapter = StoreSyncAdapter::new(
        reopened_storage,
        reopened.clone(),
        target.adapter.binding.clone(),
        Arc::new(projection),
    );
    let (tx, _) = reopened
        .prepare_import(
            &adapter,
            adapter.binding(),
            &resource,
            std::slice::from_ref(&upload.update),
            &upload.version,
        )
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let (stage_id, ready): (i64, i64) =
        sqlx::query_as("SELECT id, complete FROM sync_import_stages WHERE session_id=?")
            .bind(id)
            .fetch_one(target.storage.pool())
            .await
            .unwrap();
    assert_eq!((stage_id, ready), (first_stage.0, 1));

    // New event IDs in other sessions cannot invalidate the staged Session.
    let mut unrelated = record("other-session");
    unrelated.execution_node_id = COMPUTER.into();
    target
        .sessions
        .insert_session_record(&unrelated)
        .await
        .unwrap();
    run(&target, &unrelated.id, "other-turn").await;
    let (mut tx, batch) = reopened
        .prepare_import(
            &adapter,
            adapter.binding(),
            &resource,
            std::slice::from_ref(&upload.update),
            &upload.version,
        )
        .await
        .unwrap();
    adapter.apply(&mut *tx, &batch).await.unwrap();
    tx.commit().await.unwrap();
    source.sync.acknowledge(&upload).await.unwrap();
    let visible: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM visible_agent_events WHERE session_id=?")
            .bind(id)
            .fetch_one(target.storage.pool())
            .await
            .unwrap();
    assert_eq!(visible, 134);

    notice(&source.sessions, id, "second remote cut".into()).await;
    collect(&source.adapter, &resource).await;
    let upload = source
        .sync
        .export_pending(source.adapter.binding(), resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let batch = prepare(&target, &upload).await.unwrap();
    notice(
        &target.sessions,
        id,
        "local notice overtakes staging".into(),
    )
    .await;
    collect(&target.adapter, &resource).await;
    let mut tx = target.storage.begin_transaction().await.unwrap();
    let error = adapter.apply(&mut *tx, &batch).await.unwrap_err();
    assert_eq!(error.error_code(), talos_error::ErrorCode::Conflict);
    tx.rollback().await.unwrap();
    let visible: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM visible_agent_events WHERE session_id=?")
            .bind(id)
            .fetch_one(target.storage.pool())
            .await
            .unwrap();
    assert_eq!(visible, 135);
}
