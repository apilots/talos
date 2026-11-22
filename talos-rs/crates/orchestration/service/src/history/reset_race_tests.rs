//! Real SQLite resets between terminal selection and canonical event reduction.
use super::*;
use talos_storage::Storage;
use talos_store::session::{SessionRecord, SessionStore, SessionTreeIdentity};

async fn reset_fixture() -> (
    tempfile::TempDir,
    Arc<DatabaseSessionStore>,
    SessionHistoryService,
    Arc<CountingStorage>,
) {
    let home = tempfile::tempdir().unwrap();
    let storage = Arc::new(CountingStorage::new(
        SqliteStorage::new_with_max_connections(
            home.path().join("history.db").to_str().unwrap(),
            2,
        )
        .await
        .unwrap(),
    ));
    run_test_migrations(storage.inner.pool()).await;
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    store
        .insert_session_record(&SessionRecord {
            session_kind: talos_types::SessionKind::Standard,
            id: "session-1".into(),
            agent_id: "default".into(),
            execution_node_id: "embedded".into(),
            tree_identity: SessionTreeIdentity::Primary,
            workspace_id: "workspace".into(),
            title: "Reset race".into(),
            user_id: "user".into(),
            source_type: Default::default(),
            source_provider: Default::default(),
            channel_id: None,
            thread_id: None,
            environment: None,
            created_at: 1,
            last_activity: 1,
            message_count: 0,
            latest_turn_seq: 0,
            memory_processed_until_event_seq: 0,
            usage: Default::default(),
            conf: Default::default(),
            project_id: None,
            current_directory: None,
        })
        .await
        .unwrap();
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-1"),
        "1",
    )
    .await;
    let history = SessionHistoryService::new(Arc::new(TimelineEventService::new(store.clone())));
    (home, store, history, storage)
}

async fn barrier(pause: &test_support::TerminalReadPause) {
    tokio::time::timeout(std::time::Duration::from_secs(5), pause.reached.notified())
        .await
        .expect("history must reach the terminal selection barrier");
}

#[tokio::test]
async fn concurrent_reset_restarts_a_cursorless_history_page() {
    let (_home, store, history, storage) = reset_fixture().await;
    let pause = storage.pause_terminal_reads(1);
    let reader = tokio::spawn(async move { history.read("session-1", None, Some(10)).await });
    barrier(&pause).await;
    store.clear_session_history("session-1", 100).await.unwrap();
    pause.proceed.notify_one();
    let page = reader.await.unwrap().unwrap();
    assert_eq!(
        page,
        talos_agent_api::SessionHistoryResponse {
            data: vec![],
            next_cursor: None,
            last_stable_event_id: None,
        }
    );
}

#[tokio::test]
async fn concurrent_reset_restarts_the_internal_latest_history_window() {
    let (_home, store, history, storage) = reset_fixture().await;
    let pause = storage.pause_terminal_reads(1);
    let reader = tokio::spawn(async move {
        talos_agent::SessionHistoryReader::read(
            &history,
            SessionHistoryReadParams {
                session_id: "session-1".into(),
                window: SessionHistoryWindow::Latest,
                limit: 10,
            },
        )
        .await
    });
    barrier(&pause).await;
    store.clear_session_history("session-1", 100).await.unwrap();
    pause.proceed.notify_one();
    assert!(reader.await.unwrap().unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_reset_restarts_the_internal_zero_history_boundary() {
    let (_home, store, history, storage) = reset_fixture().await;
    let pause = storage.pause_terminal_reads(1);
    let reader = tokio::spawn(async move {
        talos_agent::SessionHistoryReader::read(
            &history,
            SessionHistoryReadParams {
                session_id: "session-1".into(),
                window: SessionHistoryWindow::OldestAfter {
                    completed_event_seq: 0,
                },
                limit: 10,
            },
        )
        .await
    });
    barrier(&pause).await;
    store.clear_session_history("session-1", 100).await.unwrap();
    pause.proceed.notify_one();
    assert!(reader.await.unwrap().unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_reset_invalidates_the_internal_nonzero_history_boundary() {
    let (_home, store, history, storage) = reset_fixture().await;
    let completed_event_seq = store
        .get_latest_agent_event_seq("session-1")
        .await
        .unwrap()
        .unwrap();
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-2"),
        "2",
    )
    .await;
    let pause = storage.pause_terminal_reads(1);
    let reader = tokio::spawn(async move {
        talos_agent::SessionHistoryReader::read(
            &history,
            SessionHistoryReadParams {
                session_id: "session-1".into(),
                window: SessionHistoryWindow::OldestAfter {
                    completed_event_seq,
                },
                limit: 10,
            },
        )
        .await
    });
    barrier(&pause).await;
    store.clear_session_history("session-1", 100).await.unwrap();
    pause.proceed.notify_one();
    assert_eq!(
        reader.await.unwrap().unwrap_err().error_code(),
        talos_error::ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn concurrent_reset_invalidates_an_existing_history_cursor() {
    let (_home, store, history, storage) = reset_fixture().await;
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-2"),
        "2",
    )
    .await;
    let cursor = history
        .read("session-1", None, Some(1))
        .await
        .unwrap()
        .next_cursor
        .unwrap();
    let pause = storage.pause_terminal_reads(1);
    let reader =
        tokio::spawn(async move { history.read("session-1", Some(&cursor), Some(10)).await });
    barrier(&pause).await;
    store.clear_session_history("session-1", 100).await.unwrap();
    pause.proceed.notify_one();
    assert_eq!(
        reader.await.unwrap().unwrap_err().error_code(),
        talos_error::ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn missing_checkpoint_tail_in_the_same_generation_still_fails() {
    let (_home, _store, history, storage) = reset_fixture().await;
    let pause = storage.pause_terminal_reads(1);
    let reader = tokio::spawn(async move { history.read("session-1", None, Some(10)).await });
    barrier(&pause).await;
    storage
        .inner
        .execute(
            "DELETE FROM agent_events WHERE session_id = ?",
            &["session-1"],
        )
        .await
        .unwrap();
    pause.proceed.notify_one();
    assert_eq!(
        reader.await.unwrap().unwrap_err().error_code(),
        talos_error::ErrorCode::InternalError
    );
}

#[tokio::test]
async fn repeated_resets_exhaust_the_bounded_history_read_attempts() {
    let (_home, store, history, storage) = reset_fixture().await;
    let pause = storage.pause_terminal_reads(3);
    let reader = tokio::spawn(async move { history.read("session-1", None, Some(10)).await });
    for generation in 1..=3 {
        barrier(&pause).await;
        store.clear_session_history("session-1", 100).await.unwrap();
        commit_completed_turn(
            store.as_ref(),
            AgentTurnRef::new("session-1", format!("turn-{}", generation + 1)),
            &format!("{}", generation + 1),
        )
        .await;
        pause.proceed.notify_one();
    }
    assert_eq!(
        reader.await.unwrap().unwrap_err().error_code(),
        talos_error::ErrorCode::Conflict
    );
}
