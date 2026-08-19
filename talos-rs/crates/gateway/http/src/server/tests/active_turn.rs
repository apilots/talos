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
            current_action_id: None,
            updated_at: 123,
        },
    );
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
    assert_eq!(payload["data"]["active_turn"]["status"], "running");
    assert_eq!(payload["data"]["active_turn"]["turn_id"], "turn-1");
    assert_eq!(
        payload["data"]["active_turn"]["last_event_id"],
        "ev_00000000000000000007"
    );
}

#[tokio::test]
async fn test_get_active_turn_handler_returns_pending_dynamic_action() {
    let event_store = Arc::new(RecordingEventStore::default());
    let action = pending_dynamic_record(
        "mock-session",
        "turn-dynamic",
        "action-dynamic",
        Some("tool-dynamic"),
        "call-dynamic",
        "ExternalTool",
    );
    event_store.user_actions.lock().unwrap().insert(
        (
            "mock-session".to_string(),
            "turn-dynamic".to_string(),
            "action-dynamic".to_string(),
        ),
        action,
    );
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-dynamic".to_string(),
            message_id: Some("msg-dynamic".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(9)),
            current_action_id: Some("action-dynamic".to_string()),
            updated_at: 125,
        },
    );
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
    assert_eq!(
        payload["data"]["active_turn"]["pending_user_action"],
        serde_json::json!({
            "actionId": "action-dynamic",
            "callId": "call-dynamic",
            "itemId": "tool-dynamic",
            "state": "pending",
            "params": {
                "type": "dynamicToolResult",
                "params": { "toolName": "ExternalTool" }
            }
        })
    );
}

#[tokio::test]
async fn test_interrupt_turn_handler_returns_success() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-1".to_string(),
            message_id: Some("msg-1".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 123,
        },
    );
    let state = test_state_with_event_store(false, event_store).await;

    let result = interrupt_turn_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(talos_types::InterruptTurnRequest {
            session_id: "mock-session".to_string(),
            turn_id: "turn-1".to_string(),
        }),
    )
    .await
    .expect("interrupt should succeed");

    let data = expect_success(result.0);
    assert!(data.success);
}
