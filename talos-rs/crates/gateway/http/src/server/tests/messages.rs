use super::*;

#[tokio::test]
async fn test_start_turn_handler_reuses_idempotent_response() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store.clone()).await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "session_id": "mock-session",
        "input": [{ "type": "text", "text": "hello" }],
        "workspace_id": null,
        "context": null,
        "project_id": null,
        "current_directory": null,
        "client_request_id": "idem-1"
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-1")
                .body(Body::from(body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("first request should succeed");
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let first_body = axum::body::to_bytes(first.into_body(), usize::MAX)
        .await
        .expect("first body should be readable");
    let first_payload: Value =
        serde_json::from_slice(&first_body).expect("first payload should be json");

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let recorded_before = event_store.events.lock().unwrap().len();

    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-1")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("second request should succeed");
    assert_eq!(second.status(), StatusCode::ACCEPTED);
    let second_body = axum::body::to_bytes(second.into_body(), usize::MAX)
        .await
        .expect("second body should be readable");
    let second_payload: Value =
        serde_json::from_slice(&second_body).expect("second payload should be json");

    assert_eq!(first_payload["status"], "success");
    assert_eq!(second_payload["status"], "success");
    assert_eq!(
        first_payload["data"]["turn_id"],
        second_payload["data"]["turn_id"]
    );
    assert_eq!(
        first_payload["data"]["message_id"],
        second_payload["data"]["message_id"]
    );
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(event_store.events.lock().unwrap().len(), recorded_before);
}

#[tokio::test]
async fn test_start_turn_handler_accepts_shared_request() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store.clone()).await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "session_id": "mock-session",
        "input": [{ "type": "text", "text": "hello" }],
        "workspace_id": null,
        "context": null,
        "project_id": null,
        "current_directory": null,
        "client_request_id": "idem-start-1"
    })
    .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-start-1")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be json");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["data"]["session_id"], "mock-session");
    assert_eq!(payload["data"]["message_id"], "msg_mock");

    let recorded = event_store.events.lock().unwrap().clone();
    assert!(!recorded.is_empty());
    assert_eq!(recorded[0].event_type, AgentEventType::TurnStarted);
    assert_eq!(recorded[0].message_id, "msg_mock");
}

#[tokio::test]
async fn test_start_turn_handler_rejects_empty_input_before_session_creation() {
    let state = test_state_with_event_store(false, Arc::new(RecordingEventStore::default())).await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "session_id": "empty-input-session",
        "input": [],
        "workspace_id": null,
        "context": {
            "source_type": "http",
            "provider": "http",
            "channel_id": "test-channel",
            "user_id": "empty-input-user",
            "thread_id": null,
            "message_id": "msg-empty-input",
            "trace_id": "trace-empty-input"
        },
        "project_id": null,
        "current_directory": null,
        "client_request_id": null
    })
    .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should return validation error");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let payload: Value = serde_json::from_slice(&response_body).expect("body should be json");
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["error"]["code"], "invalidRequest");
}

#[tokio::test]
async fn test_start_turn_handler_rejects_idempotency_hash_conflict() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let first_body = serde_json::json!({
        "session_id": "mock-session",
        "input": [{ "type": "text", "text": "hello" }],
        "workspace_id": null,
        "context": null,
        "project_id": null,
        "current_directory": null,
        "client_request_id": "idem-conflict"
    })
    .to_string();
    let second_body = serde_json::json!({
        "session_id": "mock-session",
        "input": [{ "type": "text", "text": "different" }],
        "workspace_id": null,
        "context": null,
        "project_id": null,
        "current_directory": null,
        "client_request_id": "idem-conflict"
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-conflict")
                .body(Body::from(first_body))
                .expect("request should build"),
        )
        .await
        .expect("first request should succeed");
    assert_eq!(first.status(), StatusCode::ACCEPTED);

    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-conflict")
                .body(Body::from(second_body))
                .expect("request should build"),
        )
        .await
        .expect("second request should succeed");

    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_start_turn_handler_timeout_stores_failed_idempotency() {
    let event_store = Arc::new(RecordingEventStore::default());
    let mut config = test_config();
    config.agent.kernel.timeline_events.first_event_timeout_ms = 1;
    let state = test_state_with_gateway_event_store_config(
        Arc::new(SilentGateway),
        false,
        event_store,
        config,
    )
    .await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "session_id": "mock-session",
        "input": [{ "type": "text", "text": "hello" }],
        "workspace_id": null,
        "context": null,
        "project_id": null,
        "current_directory": null,
        "client_request_id": "timeout-key"
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .header("idempotency-key", "timeout-key")
                .body(Body::from(body.clone()))
                .expect("first request should build"),
        )
        .await
        .expect("first request should return timeout");
    assert_eq!(first.status(), StatusCode::GATEWAY_TIMEOUT);

    let retry = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/start")
                .header("content-type", "application/json")
                .header("idempotency-key", "timeout-key")
                .body(Body::from(body))
                .expect("retry request should build"),
        )
        .await
        .expect("retry should return failed terminal response");
    assert_eq!(retry.status(), StatusCode::ACCEPTED);
    let retry_body = axum::body::to_bytes(retry.into_body(), usize::MAX)
        .await
        .expect("retry body should be readable");
    let retry_payload: Value =
        serde_json::from_slice(&retry_body).expect("retry payload should be json");
    assert_eq!(retry_payload["status"], "success");
    assert_eq!(retry_payload["data"]["status"], "failed_no_first_event");
    assert!(retry_payload["data"]["message_id"].is_string());
}

#[tokio::test]
async fn test_retry_turn_rejects_running_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-running".to_string(),
            message_id: Some("msg-running".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: Some(TimelineEventId::from_seq(3)),
            current_action_id: None,
            updated_at: 123,
        },
    );
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route("/v1/turns/retry", post(retry_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/retry")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "session_id": "mock-session",
                        "client_message_id": "client-1",
                        "retry_of_message_id": "msg-running",
                        "input": [{"type": "text", "text": "retry"}],
                        "workspace_id": null
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("error body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["error"]["code"], "activeTurnRunning");
}

#[tokio::test]
async fn test_retry_turn_rejects_completed_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-completed".to_string(),
            message_id: Some("msg-completed".to_string()),
            status: ActiveTurnStatus::Completed,
            last_event_id: Some(TimelineEventId::from_seq(4)),
            current_action_id: None,
            updated_at: 124,
        },
    );
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route("/v1/turns/retry", post(retry_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/retry")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "session_id": "mock-session",
                        "client_message_id": "client-1",
                        "retry_of_message_id": "msg-completed",
                        "input": [{"type": "text", "text": "retry"}],
                        "workspace_id": null
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("error body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["error"]["code"], "retrySourceCompleted");
}

#[tokio::test]
async fn test_retry_turn_allows_failed_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-failed".to_string(),
            message_id: Some("msg-failed".to_string()),
            status: ActiveTurnStatus::Failed,
            last_event_id: Some(TimelineEventId::from_seq(5)),
            current_action_id: None,
            updated_at: 125,
        },
    );
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route("/v1/turns/retry", post(retry_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let request_body = serde_json::json!({
        "session_id": "mock-session",
                        "client_message_id": "client-1",
        "retry_of_message_id": "msg-failed",
        "input": [{"type": "text", "text": "retry"}],
        "workspace_id": null
    })
    .to_string();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/retry")
                .header("content-type", "application/json")
                .header("idempotency-key", "retry-attempt-1")
                .body(Body::from(request_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["data"]["message_id"], "msg_mock");
    assert_eq!(payload["data"]["status"], "accepted");

    let retry_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/retry")
                .header("content-type", "application/json")
                .header("idempotency-key", "ignored-client-key")
                .body(Body::from(request_body))
                .expect("request should build"),
        )
        .await
        .expect("retry request should succeed");

    assert_eq!(retry_resp.status(), StatusCode::ACCEPTED);
    let retry_body = axum::body::to_bytes(retry_resp.into_body(), usize::MAX)
        .await
        .expect("retry response body should be readable");
    let retry_payload: Value =
        serde_json::from_slice(&retry_body).expect("retry payload should be valid json");
    assert_eq!(retry_payload["status"], "success");
    assert_eq!(retry_payload["data"]["turn_id"], payload["data"]["turn_id"]);
    assert_eq!(
        retry_payload["data"]["message_id"],
        payload["data"]["message_id"]
    );
}

#[tokio::test]
async fn test_submit_dynamic_tool_result_handler_accepts_shared_request() {
    let event_store = Arc::new(RecordingEventStore::default());
    let action = pending_dynamic_record(
        "mock-session",
        "turn-dynamic",
        "action-dynamic",
        Some("tool-dynamic"),
        "call-1",
        "Tool",
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
    record_pending_tool_events(
        event_store.as_ref(),
        "mock-session",
        "turn-dynamic",
        "msg-dynamic",
        "tool-dynamic",
        "call-1",
        "Tool",
    )
    .await;
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route(
            "/v1/tools/dynamic-results/submit",
            post(submit_dynamic_tool_result_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/tools/dynamic-results/submit")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "session_id": "mock-session",
                        "call_id": "call-1",
                        "tool_name": "Tool",
                        "output": "done",
                        "success": true
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["data"]["success"], true);
}

#[tokio::test]
async fn submit_user_action_handler_accepts_the_canonical_response() {
    let event_store = Arc::new(RecordingEventStore::default());
    let action = pending_dynamic_record(
        "mock-session",
        "turn-dynamic",
        "action-dynamic",
        Some("tool-dynamic"),
        "call-1",
        "Tool",
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
    record_pending_tool_events(
        event_store.as_ref(),
        "mock-session",
        "turn-dynamic",
        "msg-dynamic",
        "tool-dynamic",
        "call-1",
        "Tool",
    )
    .await;
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route("/v1/user-actions/submit", post(submit_user_action_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/user-actions/submit")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "sessionId": "mock-session",
                        "turnId": "turn-dynamic",
                        "actionId": "action-dynamic",
                        "response": {
                            "type": "dynamicToolResult",
                            "response": { "output": "done", "success": true }
                        }
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["data"]["actionId"], "action-dynamic");
    assert_eq!(payload["data"]["state"], "resuming");
}
