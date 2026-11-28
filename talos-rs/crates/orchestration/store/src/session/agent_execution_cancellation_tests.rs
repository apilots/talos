use super::*;
use crate::test_support::{ProjectTestWorkerEvents, TestWorkerEvents};

#[tokio::test]
async fn cancellation_request_preserves_the_observed_running_turn() {
    let store = store().await;
    let admitted = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let observed = admitted.active_turn.unwrap();
    let cancellation = store.begin_cancellation(&observed.identity).await.unwrap();
    let mut expected = observed;
    expected.cancellation_requested = true;
    assert_eq!(cancellation.active_turn, expected);
    assert!(!cancellation.request_id.is_empty());
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        Some(expected)
    );
}

#[tokio::test]
async fn cancellation_request_preserves_the_latest_durable_tail() {
    let store = store().await;
    store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let progress = store
        .project_test_events(TestWorkerEvents::new(vec![event(
            3,
            AgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
        )]))
        .await
        .unwrap();
    let mut expected = progress.active_turn.unwrap();
    let cancellation = store.begin_cancellation(&expected.identity).await.unwrap();
    expected.cancellation_requested = true;
    assert_eq!(cancellation.active_turn, expected);
}

#[tokio::test]
async fn rejected_request_cannot_clear_a_newer_cancellation_request() {
    let store = store().await;
    let admitted = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let observed = admitted.active_turn.unwrap();
    let first = store.begin_cancellation(&observed.identity).await.unwrap();
    let restored = store.restore_cancellation(first.clone()).await.unwrap();
    assert_eq!(restored, observed);
    let second = store.begin_cancellation(&observed.identity).await.unwrap();
    assert_ne!(first.request_id, second.request_id);
    assert!(store
        .restore_cancellation(first)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        Some(second.active_turn.clone())
    );
    assert_eq!(store.restore_cancellation(second).await.unwrap(), observed);
}

#[tokio::test]
async fn runtime_rejection_clears_request_without_overwriting_new_worker_waiting_state() {
    let store = store().await;
    let admitted = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let cancellation = store
        .begin_cancellation(&admitted.active_turn.unwrap().identity)
        .await
        .unwrap();
    let waiting = store
        .project_test_events(TestWorkerEvents::new(vec![waiting(3, "late-action")]))
        .await
        .unwrap();
    let mut expected = waiting.active_turn.unwrap();
    assert!(expected.cancellation_requested);
    assert_eq!(expected.state, ActiveTurnState::WaitingForInput);
    expected.cancellation_requested = false;
    let restored = store.restore_cancellation(cancellation).await.unwrap();
    assert_eq!(restored, expected);
    assert_eq!(restored.current_action_id.as_deref(), Some("late-action"));
}

#[tokio::test]
async fn cancellation_intent_survives_late_committed_worker_progress_and_input() {
    let store = store().await;
    let admitted = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let cancellation = store
        .begin_cancellation(&admitted.active_turn.unwrap().identity)
        .await
        .unwrap();
    let progress = store
        .project_test_events(TestWorkerEvents::new(vec![event(
            3,
            AgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
        )]))
        .await
        .unwrap();
    let input = store
        .project_test_events(TestWorkerEvents::new(user_admission(4, "message-2")))
        .await
        .unwrap();
    for observed in [progress.active_turn.unwrap(), input.active_turn.unwrap()] {
        assert_eq!(observed.state, ActiveTurnState::Running);
        assert!(observed.cancellation_requested);
    }
    let restored = store.restore_cancellation(cancellation).await.unwrap();
    assert!(!restored.cancellation_requested);
    assert_eq!(restored.last_event_seq, 4);
}

#[tokio::test]
async fn natural_terminal_wins_over_a_stale_cancellation_request() {
    let store = store().await;
    let admitted = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let identity = admitted.active_turn.unwrap().identity;
    let cancellation = store.begin_cancellation(&identity).await.unwrap();
    store
        .project_test_events(TestWorkerEvents::new(vec![event(
            3,
            AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        )]))
        .await
        .unwrap();
    assert!(store
        .restore_cancellation(cancellation)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Conflict));
    assert!(store
        .begin_cancellation(&identity)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        None
    );
}

#[tokio::test]
async fn migration_preserves_old_requests_without_exporting_them_as_worker_state() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        SqliteStorage::new(
            directory
                .path()
                .join("cancellation-migration.db")
                .to_str()
                .unwrap(),
        )
        .await
        .unwrap(),
    );
    let all = sqlx::migrate::Migrator::new(test_migrations_dir())
        .await
        .unwrap();
    let previous = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter()
                .filter(|migration| migration.version < 202611280006)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(storage.pool()).await.unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    for (session_id, payload) in [
        (
            "running",
            AgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
        ),
        (
            "waiting",
            AgentEventPayload::WaitingForInput(AgentPendingInput::Message {
                action_id: "question".into(),
                prompt: "Continue?".into(),
            }),
        ),
    ] {
        let record = crate::test_support::test_session_record(session_id);
        store.insert_session_record(&record).await.unwrap();
        let event_seq: i64 = sqlx::query_scalar("INSERT INTO agent_events(session_id,turn_id,sequence,occurred_at,persisted_at,payload_json,history_generation) VALUES (?,'turn',1,10,11,?,0) RETURNING event_seq")
            .bind(session_id).bind(serde_json::to_string(&payload).unwrap()).fetch_one(storage.pool()).await.unwrap();
        sqlx::query("INSERT INTO agent_active_turns(session_id,turn_id,state,last_event_seq,updated_at) VALUES (?,'turn','cancelling',?,99)")
            .bind(session_id).bind(event_seq).execute(storage.pool()).await.unwrap();
    }
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    for (session_id, expected) in [
        ("running", ActiveTurnState::Running),
        ("waiting", ActiveTurnState::WaitingForInput),
    ] {
        let observed = store
            .get_agent_active_turn(session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.state, expected);
        assert!(observed.cancellation_requested);
        assert_eq!(observed.updated_at, 11);
    }
}
