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

#[tokio::test]
async fn restoring_capture_exports_updates_and_tombstones_committed_without_an_adapter() {
    let source = peer(true).await;
    let target = peer(false).await;
    let projects = StorageProjectStore::new(source.storage.clone())
        .await
        .unwrap()
        .with_sync(source.adapter.clone());
    let project = projects
        .create_project_on_node(
            "Before adapter removal",
            std::path::Path::new("/worker/restored"),
            "account-1",
            "embedded",
        )
        .await
        .unwrap();
    let resource = SyncResourceKey::Project {
        project_id: project.project_id.clone(),
    };
    exchange(&source, &target, resource.clone()).await;
    let offline = StorageProjectStore::new(source.storage.clone())
        .await
        .unwrap();
    offline
        .patch_project(
            &project.project_id,
            &ProjectPatch {
                name: Some("Changed without adapter".into()),
            },
        )
        .await
        .unwrap();
    let mut read = source.storage.begin_read_transaction().await.unwrap();
    assert!(source
        .adapter
        .has_source_gap(&mut *read, &resource)
        .await
        .unwrap());
    read.commit().await.unwrap();
    let old_import = SyncImportBatch {
        scope: source.adapter.binding.scope.clone(),
        resource: resource.clone(),
        version: Default::default(),
        changes: vec![],
    };
    let rejected = source
        .adapter
        .prepare_import(&old_import)
        .await
        .unwrap_err();
    assert_eq!(rejected.error_code(), talos_error::ErrorCode::Conflict);
    exchange(&source, &target, resource.clone()).await;
    let target_projects = StorageProjectStore::new(target.storage.clone())
        .await
        .unwrap();
    assert_eq!(
        target_projects
            .get_project(&project.project_id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Changed without adapter"
    );
    offline.delete_project(&project.project_id).await.unwrap();
    exchange(&source, &target, resource).await;
    assert!(target_projects
        .get_project(&project.project_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn legacy_history_stays_local_without_new_sync_publication() {
    let source = peer(true).await;
    let id = "local-old-history";
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    source
        .storage
        .execute(
            "UPDATE sessions SET execution_architecture='legacy_read_only' WHERE session_key=?",
            &[id],
        )
        .await
        .unwrap();
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    assert!(!source.adapter.collect_revision(&resource).await.unwrap());
    let page = catalog::scan(
        source.storage.as_ref(),
        talos_sync::SyncScanParams {
            binding: source.adapter.binding.clone(),
            cursor: None,
            limit: 100,
        },
    )
    .await
    .unwrap();
    assert!(!page.resources.contains(&resource));
    assert!(source
        .sync
        .export_pending(source.adapter.binding(), resource, 100)
        .await
        .unwrap()
        .is_none());
    assert!(source
        .sessions
        .get_session_record(id)
        .await
        .unwrap()
        .is_some());
    assert!(source
        .sessions
        .ensure_worker_execution_session(id)
        .await
        .is_err());
}

#[tokio::test]
async fn deleted_session_with_pending_event_only_revision_replays_tombstone() {
    for clear_history in [false, true] {
        let mut source = peer(true).await;
        let target = peer(false).await;
        let id = "pending-event-only-deletion";
        let resource = SyncResourceKey::Session {
            session_id: id.into(),
        };
        source
            .sessions
            .insert_session_record(&record(id))
            .await
            .unwrap();
        exchange(&source, &target, resource.clone()).await;
        let mut read = source.storage.begin_read_transaction().await.unwrap();
        let stale_snapshot = source
            .sync
            .session_snapshot(&mut *read, source.adapter.binding(), id)
            .await
            .unwrap()
            .unwrap();
        read.commit().await.unwrap();
        source.sessions = DatabaseSessionStore::new(source.storage.clone())
            .await
            .unwrap();
        run(&source, id, "offline-turn").await;
        let mut read = source.storage.begin_read_transaction().await.unwrap();
        let next = read.query("SELECT changes_json FROM resource_revisions WHERE kind='session' AND resource_id=? AND revision>(SELECT COALESCE(MAX(revision),0) FROM sync_resource_capture WHERE kind='session' AND resource_id=?) ORDER BY revision LIMIT 1", &[id,id]).await.unwrap().remove(0);
        let revision: crate::capture::source::SessionRevision =
            serde_json::from_str(&next.get_string("changes_json").unwrap()).unwrap();
        assert!(revision.before.as_ref().unwrap().fields.is_none());
        assert!(revision.after.as_ref().unwrap().fields.is_none());
        read.commit().await.unwrap();
        if clear_history {
            source.sessions.clear_session_history(id, 30).await.unwrap();
            run(&source, id, "offline-next-generation-turn").await;
        }
        source.sessions.delete_session_tree(id).await.unwrap();
        let mut read = source.storage.begin_read_transaction().await.unwrap();
        assert!(
            source
                .adapter
                .has_source_gap(&mut *read, &resource)
                .await
                .unwrap(),
            "pending event-only revisions and deletion must fence imports"
        );
        read.commit().await.unwrap();
        let stale = SyncImportBatch {
            scope: source.adapter.binding.scope.clone(),
            resource: resource.clone(),
            version: Default::default(),
            changes: vec![SyncChange::Metadata {
                before: None,
                after: Box::new(SyncResourceSnapshot::Session(Box::new(stale_snapshot))),
            }],
        };
        assert_eq!(
            source
                .adapter
                .prepare_import(&stale)
                .await
                .unwrap_err()
                .error_code(),
            talos_error::ErrorCode::Conflict
        );
        let mut write = source.storage.begin_transaction().await.unwrap();
        assert_eq!(
            source
                .adapter
                .apply(&mut *write, &stale)
                .await
                .unwrap_err()
                .error_code(),
            talos_error::ErrorCode::Conflict
        );
        write.rollback().await.unwrap();
        assert!(source
            .sessions
            .get_session_record(id)
            .await
            .unwrap()
            .is_none());
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
        source.adapter = Arc::new(StoreSyncAdapter::new(
            storage.clone(),
            sync.clone(),
            source.adapter.binding.clone(),
            Arc::new(projection),
        ));
        source.sessions = DatabaseSessionStore::new(storage.clone()).await.unwrap();
        source.storage = storage;
        source.sync = sync;
        exchange(&source, &target, resource).await;
        assert!(
            target
                .sessions
                .get_session_record(id)
                .await
                .unwrap()
                .is_none(),
            "restored capture must send the offline Session deletion"
        );
    }
}

#[tokio::test]
async fn recreated_deleted_session_cannot_revive_a_retired_sync_resource() {
    let mut source = peer(true).await;
    let target = peer(false).await;
    let id = "recreated-offline-session";
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    exchange(&source, &target, resource.clone()).await;
    source.sessions = DatabaseSessionStore::new(source.storage.clone())
        .await
        .unwrap();
    run(&source, id, "retired-turn").await;
    source.sessions.delete_session_tree(id).await.unwrap();
    // Direct Store insertion bypasses product retirement; Sync must retain its tombstone.
    let mut recreated = record(id);
    recreated.title = "Recreated Session".into();
    source
        .sessions
        .insert_session_record(&recreated)
        .await
        .unwrap();
    run(&source, id, "current-turn").await;
    let events = source
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    assert_eq!(events.len(), 4);
    assert!(events
        .iter()
        .all(|entry| entry.event.identity().turn_id == "current-turn"));
    collect(&source.adapter, &resource).await;
    let Err(error) = source
        .sync
        .export_pending(source.adapter.binding(), resource, 100)
        .await
    else {
        panic!("A permanent Sync tombstone must reject Session ID revival.");
    };
    assert_eq!(error.error_code(), talos_error::ErrorCode::Conflict);
    assert_eq!(
        target
            .sessions
            .get_session_record(id)
            .await
            .unwrap()
            .unwrap()
            .title,
        "Original"
    );
}

#[tokio::test]
async fn event_only_revision_requires_a_matching_retired_execution_generation() {
    let invalidations = [
        "UPDATE resource_revisions SET owner_user_id='foreign-owner'",
        "UPDATE resource_revisions SET execution_node_id='foreign-node'",
        "UPDATE resource_revisions SET changes_json=replace(changes_json,'\"generation\":0','\"generation\":1')",
        "UPDATE resource_revisions SET changes_json=replace(changes_json,'\"execution_architecture\":\"worker\"','\"execution_architecture\":\"legacy_read_only\"')",
        "UPDATE resource_revisions SET changes_json=replace(changes_json,'\"execution_architecture\":\"worker\"','\"execution_architecture\":\"unknown\"')",
        "UPDATE resource_revisions SET changes_json=json_remove(changes_json,'$.before.fields.execution_architecture','$.after.fields.execution_architecture')",
        "DELETE FROM session_event_retention WHERE session_id=?",
    ];
    for invalidate in invalidations {
        let mut source = peer(true).await;
        let target = peer(false).await;
        let id = "ineligible-retired-session";
        let resource = SyncResourceKey::Session {
            session_id: id.into(),
        };
        source
            .sessions
            .insert_session_record(&record(id))
            .await
            .unwrap();
        exchange(&source, &target, resource.clone()).await;
        source.sessions = DatabaseSessionStore::new(source.storage.clone())
            .await
            .unwrap();
        run(&source, id, "offline-turn").await;
        source.sessions.delete_session_tree(id).await.unwrap();
        let sql = if invalidate.starts_with("UPDATE") {
            format!("{invalidate} WHERE kind='session' AND resource_id=? AND (json_type(changes_json,'$.before.fields')='object' OR json_type(changes_json,'$.after.fields')='object')")
        } else {
            invalidate.into()
        };
        source.storage.execute(&sql, &[id]).await.unwrap();
        let mut read = source.storage.begin_read_transaction().await.unwrap();
        assert!(
            !source
                .adapter
                .has_source_gap(&mut *read, &resource)
                .await
                .unwrap(),
            "invalid deletion proof: {invalidate}"
        );
        read.commit().await.unwrap();
        assert!(
            !source.adapter.collect_revision(&resource).await.unwrap(),
            "invalid deletion proof: {invalidate}"
        );
        assert!(source
            .sync
            .export_pending(source.adapter.binding(), resource, 100)
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn deleted_imported_replica_publishes_its_tombstone_without_capture_echo() {
    let source = peer(true).await;
    let replica = peer(false).await;
    let id = "deleted-imported-replica";
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    exchange(&source, &replica, resource.clone()).await;
    replica.sessions.delete_session_tree(id).await.unwrap();
    exchange(&replica, &source, resource).await;
    assert!(source
        .sessions
        .get_session_record(id)
        .await
        .unwrap()
        .is_none());
    assert!(source
        .sync
        .pending_resources(source.adapter.binding(), 100)
        .await
        .unwrap()
        .is_empty());
    let latest = source.storage.query(
        "SELECT MAX(revision) AS revision FROM resource_revisions WHERE kind='session' AND resource_id=?", &[id]
    ).await.unwrap().remove(0).get_i64("revision").unwrap();
    let captured = source.storage.query(
        "SELECT MAX(revision) AS revision FROM sync_resource_capture WHERE kind='session' AND resource_id=?", &[id]
    ).await.unwrap().remove(0).get_i64("revision").unwrap();
    assert_eq!(latest, captured);
}

#[tokio::test]
async fn rebuilding_a_missing_projected_tail_keeps_immutable_sync_timestamps() {
    let source = peer(true).await;
    let target = peer(false).await;
    let id = "rebuilt-sync-tail";
    let resource = SyncResourceKey::Session {
        session_id: id.into(),
    };
    source
        .sessions
        .insert_session_record(&record(id))
        .await
        .unwrap();
    let events = vec![
        event(
            id,
            "turn-1",
            1,
            AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        ),
        event(
            id,
            "turn-1",
            2,
            AgentEventPayload::TurnFailed(AgentTurnFailed {
                error: talos_error::ApiError::with_message(
                    talos_error::ErrorCode::ProviderUnavailable,
                    "Provider became unavailable",
                ),
                retryable: true,
            }),
        ),
    ];
    let facts: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(index, event)| agent_protocol::AgentCommittedEvent {
            cursor: agent_protocol::AgentSessionEventCursor {
                session_id: id.into(),
                generation: 0,
                offset: index as i64 + 1,
            },
            settlement: event.payload.is_terminal().then(|| {
                agent_protocol::AgentExecutionSettlement::Finished {
                    metrics: AgentRunMetrics::default(),
                    memory_usage: vec![],
                }
            }),
            event,
            execution_purpose: agent_protocol::AgentExecutionPurpose::Turn,
            history_supplement: None,
            automation_input: None,
            command: None,
            execution_model: None,
        })
        .collect();
    let page = agent_protocol::AgentSessionEventsResult {
        events: facts,
        next_cursor: agent_protocol::AgentSessionEventCursor {
            session_id: id.into(),
            generation: 0,
            offset: 2,
        },
        has_more: false,
        active_turn: None,
    };
    let commit = source
        .sessions
        .project_worker_events(WorkerProjectionParams {
            execution_node_id: "embedded".into(),
            session_id: id.into(),
            after_cursor: None,
            page: page.clone(),
        })
        .await
        .unwrap();
    source
        .sessions
        .complete_worker_settlement(&commit.cursor)
        .await
        .unwrap();
    source
        .storage
        .execute(
            "UPDATE agent_events SET persisted_at=100 WHERE session_id=?",
            &[id],
        )
        .await
        .unwrap();
    source
        .storage
        .execute(
            "UPDATE worker_event_projection SET product_persisted_at=100 WHERE session_id=?",
            &[id],
        )
        .await
        .unwrap();
    exchange(&source, &target, resource.clone()).await;
    let before = target
        .sessions
        .list_timeline_events_after(id, 0, 100)
        .await
        .unwrap();
    source
        .storage
        .execute(
            "DELETE FROM agent_events WHERE session_id=? AND sequence=2",
            &[id],
        )
        .await
        .unwrap();
    let request = source
        .sessions
        .begin_worker_projection_rebuild(WorkerProjectionRebuildParams {
            session_id: id.into(),
            execution_node_id: "embedded".into(),
            history_generation: 0,
        })
        .await
        .unwrap();
    source
        .sessions
        .stage_worker_projection_rebuild(
            &request,
            WorkerProjectionParams {
                execution_node_id: "embedded".into(),
                session_id: id.into(),
                after_cursor: None,
                page,
            },
        )
        .await
        .unwrap();
    source
        .sessions
        .finish_worker_projection_rebuild(&request)
        .await
        .unwrap();
    exchange(&source, &target, resource).await;
    assert_eq!(
        target
            .sessions
            .list_timeline_events_after(id, 0, 100)
            .await
            .unwrap(),
        before
    );
    assert_eq!(
        source
            .sessions
            .list_timeline_events_after(id, 0, 100)
            .await
            .unwrap(),
        before
    );
}
