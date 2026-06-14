use super::*;

#[tokio::test]
async fn test_submit_message_command_returns_async_ack() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store.clone());

    let request = TalosMessageRequest {
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        workspace_id: None,
        context: None,
        model: None,
        project_id: None,
        current_directory: None,
    };

    let result = submit_message_command(
        state,
        test_auth(),
        HeaderMap::new(),
        "mock-session".to_string(),
        request,
    )
    .await
    .expect("message should be processed");

    assert_eq!(result.session_id, "mock-session");
    assert_eq!(result.message_id.as_deref(), Some("msg_mock"));
    assert_eq!(result.status, IdempotencyStatus::Accepted);
    assert!(result.turn_id.starts_with("turn_"));

    let recorded = event_store.events.lock().unwrap().clone();
    assert!(!recorded.is_empty());
    assert_eq!(recorded[0].event_type, TalosTimelineEventType::TurnStarted);
    assert_eq!(recorded[0].message_id, "msg_mock");
}

#[tokio::test]
async fn test_session_message_handler_reuses_idempotent_ack() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store.clone());
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/messages",
            post(session_message_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "input": [{ "type": "text", "text": "hello" }]
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/messages")
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
                .uri("/v1/sessions/mock-session/messages")
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

    assert_eq!(first_payload["turn_id"], second_payload["turn_id"]);
    assert_eq!(first_payload["message_id"], second_payload["message_id"]);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(event_store.events.lock().unwrap().len(), recorded_before);
}

#[tokio::test]
async fn test_session_message_handler_rejects_empty_input_before_session_creation() {
    let state = test_state_with_event_store(false, Arc::new(RecordingEventStore::default()));
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/messages",
            post(session_message_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "input": [],
        "context": {
            "source_type": "http",
            "provider": "http",
            "channel_id": "test-channel",
            "user_id": "empty-input-user",
            "thread_id": null,
            "message_id": "msg-empty-input",
            "trace_id": "trace-empty-input"
        }
    })
    .to_string();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/empty-input-session/messages")
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
    assert!(payload
        .to_string()
        .contains("input must include at least one non-empty text item"));
}

#[tokio::test]
async fn test_session_message_handler_rejects_idempotency_hash_conflict() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/messages",
            post(session_message_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let first_body = serde_json::json!({
        "input": [{ "type": "text", "text": "hello" }]
    })
    .to_string();
    let second_body = serde_json::json!({
        "input": [{ "type": "text", "text": "different" }]
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/messages")
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
                .uri("/v1/sessions/mock-session/messages")
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
async fn test_submit_message_command_timeout_stores_failed_idempotency() {
    let event_store = Arc::new(RecordingEventStore::default());
    let mut config = test_config();
    config.agent.kernel.timeline_events.first_event_timeout_ms = 1;
    let state = test_state_with_gateway_event_store_config(
        Arc::new(SilentGateway),
        false,
        event_store,
        config,
    );
    let request = TalosMessageRequest {
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        workspace_id: None,
        context: None,
        model: None,
        project_id: None,
        current_directory: None,
    };
    let mut headers = HeaderMap::new();
    headers.insert("idempotency-key", "timeout-key".parse().unwrap());

    let err = submit_message_command(
        state.clone(),
        test_auth(),
        headers.clone(),
        "mock-session".to_string(),
        request.clone(),
    )
    .await
    .expect_err("first request should timeout");
    assert_eq!(err.into_response().status(), StatusCode::GATEWAY_TIMEOUT);

    let retry = submit_message_command(
        state,
        test_auth(),
        headers,
        "mock-session".to_string(),
        request,
    )
    .await
    .expect("retry should return failed terminal ack");
    assert_eq!(retry.status, IdempotencyStatus::FailedNoFirstEvent);
    assert!(retry.message_id.is_some());
}

#[tokio::test]
async fn test_retry_message_attempt_rejects_running_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-running".to_string(),
            message_id: Some("msg-running".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: Some(TimelineEventId::from_seq(3)),
            pending_user_action: None,
            updated_at: 123,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/message-attempts",
            post(retry_message_attempt_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/message-attempts")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
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
    assert_eq!(payload["code"], "retry_active_turn_running");
}

#[tokio::test]
async fn test_retry_message_attempt_rejects_completed_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-completed".to_string(),
            message_id: Some("msg-completed".to_string()),
            status: ActiveTurnStatus::Completed,
            last_event_id: Some(TimelineEventId::from_seq(4)),
            pending_user_action: None,
            updated_at: 124,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/message-attempts",
            post(retry_message_attempt_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/message-attempts")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
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
    assert_eq!(payload["code"], "retry_source_completed");
}

#[tokio::test]
async fn test_retry_message_attempt_allows_failed_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-failed".to_string(),
            message_id: Some("msg-failed".to_string()),
            status: ActiveTurnStatus::Failed,
            last_event_id: Some(TimelineEventId::from_seq(5)),
            pending_user_action: None,
            updated_at: 125,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/message-attempts",
            post(retry_message_attempt_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let request_body = serde_json::json!({
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
                .uri("/v1/sessions/mock-session/message-attempts")
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
        .expect("ack body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["message_id"], "msg_mock");
    assert_eq!(payload["status"], "accepted");

    let retry_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/message-attempts")
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
        .expect("retry ack body should be readable");
    let retry_payload: Value =
        serde_json::from_slice(&retry_body).expect("retry payload should be valid json");
    assert_eq!(retry_payload["turn_id"], payload["turn_id"]);
    assert_eq!(retry_payload["message_id"], payload["message_id"]);
}
