use super::*;
use talos_agent_api::{ActiveTurnState, AgentTurnRef};

#[tokio::test]
async fn waiting_interrupt_discards_usage_without_dispatching_the_queue() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, queue) = test_service_with_storage(
        execution.clone(),
        Arc::new(RecordingAgentCoordinator::default()),
        storage.clone(),
    )
    .await;
    create_test_session_record(store.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), None).await;
    let profile = DatabaseProfileStore::new(storage.pool().clone());
    let session = test_session();
    profile
        .record_turn_usage(talos_store::profile::TurnUsageUpdate {
            user_id: session.user_id.clone(),
            session_id: session.id.clone(),
            turn_id: "turn-original".to_string(),
            request_id: "request-waiting".to_string(),
            model_ref: session.conf.model_selection.clone().unwrap().model_ref,
            usage: talos_types::TokenUsage {
                input_tokens: 2,
                total_tokens: 2,
                ..Default::default()
            },
            usage_available: true,
            stop_reason: StopReason::WaitingUserAction,
            timezone: "UTC".to_string(),
            completed_at_ms: 1000,
            usage_date: "2026-09-05".to_string(),
        })
        .await
        .unwrap();
    queue.add(&session.id, test_user_input("next")).unwrap();
    let outcome = service
        .interrupt_canonical_turn(&session, "turn-original")
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM profile_turn_usage WHERE session_id = 's1'")
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(
        (
            outcome,
            count,
            queue.list("s1").unwrap().messages.len(),
            execution.stream_requests.lock().unwrap().len()
        ),
        (AgentInterruptOutcome::Cancelled, 0, 1, 0),
    );
    assert!(store.get_agent_active_turn("s1").await.unwrap().is_none());
}

#[tokio::test]
async fn interrupt_admits_the_exact_canonical_turn_before_harness_control() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let coordinator = Arc::new(RecordingAgentCoordinator::default());
    let (service, _event_service, store, _message_queue) =
        test_service_with_coordinator(agent_execution, coordinator.clone()).await;
    create_test_session_record(store.clone()).await;
    let identity = AgentTurnRef::new("s1", "turn-canonical");
    let input = completed_agent_user_input(
        "turn-canonical",
        "message-canonical",
        test_user_input("run"),
        1,
    )
    .unwrap();
    commit_test_running_turn(store.as_ref(), identity.clone(), input).await;

    let mut session = test_session();
    session.conf.model_selection = None;
    let outcome = service
        .interrupt_canonical_turn(&session, &identity.turn_id)
        .await
        .unwrap();
    let cancelled_turns = coordinator.cancelled_turns.lock().unwrap().clone();
    let active_turn = store
        .get_agent_active_turn(&identity.session_id)
        .await
        .unwrap();
    let events = store.list_agent_turn_events(&identity, 32).await.unwrap();

    assert_eq!(outcome, AgentInterruptOutcome::Cancelled);
    assert_eq!(
        (cancelled_turns, active_turn),
        (vec![identity.turn_id.clone()], None)
    );
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(CanonicalAgentEventPayload::TurnFailed(failed))
            if failed.error.code == talos_error::ErrorCode::Cancelled
    ));
}

#[tokio::test]
async fn rejected_harness_interrupt_restores_the_canonical_running_state() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let coordinator = Arc::new(RecordingAgentCoordinator {
        interrupt_error: Mutex::new(Some(TalosError::public(
            talos_error::ErrorCode::Conflict,
            "Harness rejected interruption",
        ))),
        ..Default::default()
    });
    let (service, _event_service, store, _message_queue) =
        test_service_with_coordinator(agent_execution, coordinator).await;
    create_test_session_record(store.clone()).await;
    let identity = AgentTurnRef::new("s1", "turn-rejected");
    let input = completed_agent_user_input(
        "turn-rejected",
        "message-rejected",
        test_user_input("run"),
        1,
    )
    .unwrap();
    commit_test_running_turn(store.as_ref(), identity.clone(), input).await;

    let error = service
        .interrupt_canonical_turn(&test_session(), &identity.turn_id)
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        store
            .get_agent_active_turn(&identity.session_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        ActiveTurnState::Running
    );
}
