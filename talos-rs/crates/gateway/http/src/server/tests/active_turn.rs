use super::*;

#[tokio::test]
async fn test_get_active_turn_handler_returns_active_state() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-1".to_string(),
            message_id: Some("msg-1".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: Some(TimelineEventId::from_seq(7)),
            pending_user_action: None,
            updated_at: 123,
        },
    );
    let state = test_state_with_event_store(false, event_store);
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
    assert_eq!(payload["active_turn"]["status"], "running");
    assert_eq!(payload["active_turn"]["turn_id"], "turn-1");
    assert_eq!(
        payload["active_turn"]["last_event_id"],
        "ev_00000000000000000007"
    );
}

#[tokio::test]
async fn test_get_active_turn_handler_returns_pending_dynamic_action() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-dynamic".to_string(),
            message_id: Some("msg-dynamic".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(9)),
            pending_user_action: Some(PendingUserAction::DynamicToolResult(
                PendingDynamicToolResult {
                    item_id: Some("tool-dynamic".to_string()),
                    call_id: "call-dynamic".to_string(),
                    tool_name: "ExternalTool".to_string(),
                },
            )),
            updated_at: 125,
        },
    );
    let state = test_state_with_event_store(false, event_store);
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
    assert_eq!(payload["active_turn"]["status"], "waiting_user_action");
    assert_eq!(
        payload["active_turn"]["pending_user_action"]["type"],
        "dynamic_tool_result"
    );
    assert_eq!(
        payload["active_turn"]["pending_user_action"]["call_id"],
        "call-dynamic"
    );
    assert_eq!(
        payload["active_turn"]["pending_user_action"]["tool_name"],
        "ExternalTool"
    );
}

#[tokio::test]
async fn test_interrupt_turn_handler_returns_success() {
    let state = test_state(false);

    let result = interrupt_turn_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(talos_types::InterruptTurnRequest {
            session_id: "mock-session".to_string(),
        }),
    )
    .await
    .expect("interrupt should succeed");

    assert!(result.0.success);
}
