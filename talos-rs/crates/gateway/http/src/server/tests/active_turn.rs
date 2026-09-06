use super::*;

#[tokio::test]
async fn test_get_active_turn_handler_returns_active_state() {
    let event_store = Arc::new(RecordingAgentExecutionStore::default());
    event_store.set_active_turn(AgentActiveTurnRecord {
        identity: AgentTurnRef::new("mock-session", "turn-1"),
        initial_message_id: Some("msg-1".to_string()),
        state: ActiveTurnState::Running,
        last_event_seq: 7,
        current_action_id: None,
        updated_at: 123,
    });
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/active-turn",
            get(get_active_turn_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/active-turn")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("active turn body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["data"]["activeTurn"]["state"], "running");
    assert_eq!(
        payload["data"]["activeTurn"]["identity"]["turnId"],
        "turn-1"
    );
    assert_eq!(
        payload["data"]["activeTurn"]["lastEventId"],
        "ev_00000000000000000007"
    );
}

#[tokio::test]
async fn test_get_active_turn_handler_returns_waiting_state_without_a_second_action_dto() {
    let event_store = Arc::new(RecordingAgentExecutionStore::default());
    event_store.set_active_turn(AgentActiveTurnRecord {
        identity: AgentTurnRef::new("mock-session", "turn-dynamic"),
        initial_message_id: Some("msg-dynamic".to_string()),
        state: ActiveTurnState::WaitingForInput,
        last_event_seq: 9,
        current_action_id: Some("action-dynamic".to_string()),
        updated_at: 125,
    });
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/active-turn",
            get(get_active_turn_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/active-turn")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("active turn body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["data"]["activeTurn"]["state"], "waitingForInput");
    assert!(payload["data"]["activeTurn"]
        .get("pendingUserAction")
        .is_none());
}

#[tokio::test]
async fn test_interrupt_turn_handler_returns_success() {
    let execution_store = test_agent_execution_store().await;
    seed_running_agent_execution(execution_store.as_ref(), "mock-session", "turn-1").await;
    let state = test_state_with_event_store_and_agent_execution_store(
        false,
        execution_store.clone(),
        execution_store,
    )
    .await;

    let result = interrupt_turn_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(talos_agent_api::InterruptTurnRequest {
            identity: talos_types::AgentTurnRef::new("mock-session", "turn-1"),
        }),
    )
    .await
    .expect("interrupt should succeed");

    let data = expect_success(result.0);
    assert_eq!(data.identity.turn_id, "turn-1");
    assert!(matches!(
        data.outcome,
        talos_agent_api::AgentInterruptOutcome::Cancelled
            | talos_agent_api::AgentInterruptOutcome::Completed
    ));
}
