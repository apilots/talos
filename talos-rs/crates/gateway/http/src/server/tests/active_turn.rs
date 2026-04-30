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
            last_event_id: Some(SessionEventId::from_seq(7)),
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
    assert_eq!(payload["status"], "running");
    assert_eq!(payload["turn_id"], "turn-1");
    assert_eq!(payload["last_event_id"], "ev_00000000000000000007");
}

#[tokio::test]
async fn test_get_active_turn_handler_returns_pending_approval_action() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-approval".to_string(),
            message_id: Some("msg-approval".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(SessionEventId::from_seq(8)),
            pending_user_action: Some(PendingUserAction::ToolApproval(PendingToolApproval {
                call_id: "call-approval".to_string(),
                tool_name: "Bash".to_string(),
                prompt: "Approve command?".to_string(),
                arguments: serde_json::json!({"cmd": "date"}).into(),
            })),
            updated_at: 124,
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
    assert_eq!(payload["status"], "waiting_user_action");
    assert_eq!(payload["pending_user_action"]["type"], "tool_approval");
    assert_eq!(payload["pending_user_action"]["call_id"], "call-approval");
    assert_eq!(payload["pending_user_action"]["tool_name"], "Bash");
    assert_eq!(payload["pending_user_action"]["prompt"], "Approve command?");
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
            last_event_id: Some(SessionEventId::from_seq(9)),
            pending_user_action: Some(PendingUserAction::DynamicToolResult(
                PendingDynamicToolResult {
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
    assert_eq!(payload["status"], "waiting_user_action");
    assert_eq!(
        payload["pending_user_action"]["type"],
        "dynamic_tool_result"
    );
    assert_eq!(payload["pending_user_action"]["call_id"], "call-dynamic");
    assert_eq!(payload["pending_user_action"]["tool_name"], "ExternalTool");
}

#[tokio::test]
async fn test_cancel_active_turn_handler_returns_success() {
    let state = test_state(false);

    let result = cancel_active_turn_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("cancel should succeed");

    assert_eq!(result.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_submit_tool_approval_handler_returns_no_content() {
    let state = test_state(false);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/tool-approvals",
            post(submit_tool_approval_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/tool-approvals")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "call_id": "call-1",
                        "tool_name": "Bash",
                        "approved": false,
                        "reason": "deny"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_submit_tool_approval_handler_maps_missing_session_to_not_found() {
    let state = test_state(false);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/tool-approvals",
            post(submit_tool_approval_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/missing-session/tool-approvals")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "call_id": "call-1",
                        "tool_name": "Bash",
                        "approved": false,
                        "reason": "deny"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
