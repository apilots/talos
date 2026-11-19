use super::*;

#[tokio::test]
async fn cleared_history_is_hidden_and_reclaimed_only_after_capture() {
    let source = peer(true).await;
    let id = "clear-before-capture";
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    run(&source, id, "retired-turn").await;
    assert!(source.sessions.clear_session_history(id, 30).await.unwrap());
    assert!(source
        .sessions
        .get_latest_agent_terminal_event(id)
        .await
        .unwrap()
        .is_none());
    assert!(!reclaim::retired_history(&source.adapter).await.unwrap());
    let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE session_id=?")
        .bind(id)
        .fetch_one(source.storage.pool())
        .await
        .unwrap();
    assert_eq!(retained, 4);

    collect(&source.adapter, &resource).await;
    assert!(reclaim::retired_history(&source.adapter).await.unwrap());
    let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE session_id=?")
        .bind(id)
        .fetch_one(source.storage.pool())
        .await
        .unwrap();
    assert_eq!(retained, 0);
    assert!(source
        .sync
        .export_pending(source.adapter.binding(), resource, 128)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn outbox_failure_preserves_committed_session_and_replays_after_reopening() {
    let source = peer(true).await;
    let id = "durable-capture";
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    source
        .storage
        .execute(
            "CREATE TRIGGER reject_capture BEFORE INSERT ON sync_outbox
         BEGIN SELECT RAISE(ABORT, 'test outbox failure'); END",
            &[],
        )
        .await
        .unwrap();

    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    run(&source, id, "completed-before-restart").await;
    let projects = StorageProjectStore::new(source.storage.clone())
        .await
        .unwrap()
        .with_sync(source.adapter.clone());
    let project = projects
        .create_project_on_node(
            "Committed while collection fails",
            std::path::Path::new("/worker/project"),
            "account-1",
            "embedded",
        )
        .await
        .unwrap();
    let project_resource = SyncResourceKey::Project {
        project_id: project.project_id,
    };
    assert!(source.adapter.collect_revision(&resource).await.is_err());
    assert!(source
        .adapter
        .collect_revision(&project_resource)
        .await
        .is_err());
    assert!(source
        .sync
        .pending_resources(source.adapter.binding(), 10)
        .await
        .unwrap()
        .is_empty());
    let watermark: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_resource_capture WHERE kind='session' AND resource_id=?",
    )
    .bind(id)
    .fetch_one(source.storage.pool())
    .await
    .unwrap();
    assert_eq!(watermark, 0);
    assert!(
        source
            .sync
            .pending_changes(source.adapter.binding())
            .await
            .unwrap()
            > 0,
        "a failed capture still leaves pending work"
    );

    let storage = Arc::new(
        SqliteStorage::new(source._home.path().join("facts.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    let sync = Arc::new(
        TalosSync::open(
            storage.clone(),
            SyncOptions {
                hub_endpoint: "wss://sync.example.test/v1/sync".into(),
            },
        )
        .await
        .unwrap(),
    );
    let sessions = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let adapter = Arc::new(StoreSyncAdapter::new(
        storage.clone(),
        sync.clone(),
        source.adapter.binding.clone(),
        Arc::new(projection),
    ));
    let terminal = sessions
        .get_latest_agent_terminal_event(id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(terminal.identity.turn_id, "completed-before-restart");
    assert!(sessions.get_agent_active_turn(id).await.unwrap().is_none());
    storage
        .execute("DROP TRIGGER reject_capture", &[])
        .await
        .unwrap();

    adapter.start_capture();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let caught_up: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM sync_resource_capture c WHERE c.kind='session' AND c.resource_id=?1 AND c.revision =
                        (SELECT MAX(revision) FROM resource_revisions r WHERE r.kind='session' AND r.resource_id=?1)
                )",
            )
            .bind(id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
            if caught_up == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!adapter.collect_revision(&resource).await.unwrap());
    let retained_revisions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM resource_revisions WHERE kind='session' AND resource_id=?",
    )
    .bind(id)
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(retained_revisions, 1);
    let first = sync
        .export_pending(adapter.binding(), resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    sync.acknowledge(&first).await.unwrap();
    assert!(!adapter.collect_revision(&resource).await.unwrap());
    assert!(sync
        .export_pending(adapter.binding(), resource, 128)
        .await
        .unwrap()
        .is_none());
    let mapped: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_event_map WHERE session_id=?")
        .bind(id)
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(mapped, 4);
    let project = sync
        .export_pending(adapter.binding(), project_resource, 128)
        .await
        .unwrap()
        .unwrap();
    assert!(sync.pending_changes(adapter.binding()).await.unwrap() > 0);
    sync.acknowledge(&project).await.unwrap();
    assert_eq!(sync.pending_changes(adapter.binding()).await.unwrap(), 0);
}
