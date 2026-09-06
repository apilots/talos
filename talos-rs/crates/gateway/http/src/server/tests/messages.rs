use super::*;

#[tokio::test]
async fn test_start_turn_handler_reuses_idempotent_response() {
    let event_store = test_agent_execution_store().await;
    let state = test_state_with_event_store_and_agent_execution_store(
        false,
        event_store.clone(),
        event_store.clone(),
    )
    .await;
    let mut events = state.event_service.subscribe_live_events();
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "sessionId": "mock-session",
        "input": { "content": [{ "type": "text", "text": "hello" }] },
        "projectId": null,
        "currentDirectory": null,
        "clientRequestId": "idem-1"
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

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let event = events
                .recv()
                .await
                .expect("turn events should remain available");
            if matches!(
                event.event.agent_event().expect("Agent fixture").payload,
                talos_agent_api::AgentEventPayload::TurnCompleted(_)
                    | talos_agent_api::AgentEventPayload::TurnFailed(_)
            ) {
                break;
            }
        }
    })
    .await
    .expect("the first turn must settle before checking idempotency");
    let recorded_before = event_store
        .list_agent_events_after("mock-session", None, 100)
        .await
        .unwrap();

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
        first_payload["data"]["turnId"],
        second_payload["data"]["turnId"]
    );
    assert_eq!(
        first_payload["data"]["messageId"],
        second_payload["data"]["messageId"]
    );
    assert_eq!(
        event_store
            .list_agent_events_after("mock-session", None, 100)
            .await
            .unwrap(),
        recorded_before
    );
}

#[tokio::test]
async fn test_start_turn_handler_accepts_shared_request() {
    let event_store = test_agent_execution_store().await;
    let state = test_state_with_event_store_and_agent_execution_store(
        false,
        event_store.clone(),
        event_store.clone(),
    )
    .await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "sessionId": "mock-session",
        "input": { "content": [{ "type": "text", "text": "hello" }] },
        "projectId": null,
        "currentDirectory": null,
        "clientRequestId": "idem-start-1"
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
    assert_eq!(payload["data"]["sessionId"], "mock-session");
    let message_id = payload["data"]["messageId"]
        .as_str()
        .expect("message_id should be a string");
    assert!(message_id.starts_with("msg_"));

    let recorded = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let recorded = event_store
                .list_agent_events_after("mock-session", None, 100)
                .await
                .unwrap();
            if !recorded.is_empty() {
                break recorded;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("accepted Turn should reach the Timeline projection");
    assert!(matches!(
        recorded[0].payload,
        CanonicalAgentEventPayload::TurnStarted(_)
    ));
    assert!(recorded.iter().any(|record| matches!(
        &record.payload,
        CanonicalAgentEventPayload::MessageCompleted(message)
            if message.message_id == message_id
    )));
}

#[tokio::test]
async fn test_start_turn_handler_rejects_empty_input_before_session_creation() {
    let state =
        test_state_with_event_store(false, Arc::new(RecordingAgentExecutionStore::default())).await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "sessionId": "empty-input-session",
        "input": { "content": [] },
        "projectId": null,
        "currentDirectory": null,
        "clientRequestId": null
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
    let event_store = Arc::new(RecordingAgentExecutionStore::default());
    let state = test_state_with_event_store(false, event_store).await;
    let app = Router::new()
        .route("/v1/turns/start", post(start_turn_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let first_body = serde_json::json!({
        "sessionId": "mock-session",
        "input": { "content": [{ "type": "text", "text": "hello" }] },
        "projectId": null,
        "currentDirectory": null,
        "clientRequestId": "idem-conflict"
    })
    .to_string();
    let second_body = serde_json::json!({
        "sessionId": "mock-session",
        "input": { "content": [{ "type": "text", "text": "different" }] },
        "projectId": null,
        "currentDirectory": null,
        "clientRequestId": "idem-conflict"
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
async fn test_retry_turn_rejects_running_active_turn() {
    let event_store = test_agent_execution_store().await;
    seed_running_agent_execution(event_store.as_ref(), "mock-session", "turn-running").await;
    let state = test_state_with_event_store_and_agent_execution_store(
        false,
        event_store.clone(),
        event_store,
    )
    .await;
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
                        "sessionId": "mock-session",
                        "clientMessageId": "client-1",
                        "retryOfMessageId": "msg-running",
                        "input": {"content": [{"type": "text", "text": "retry"}]},
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
async fn submit_turn_input_handler_resumes_a_waiting_user_action() {
    let action = pending_dynamic_action("action-dynamic", "call-1", "Tool");
    let event_store = test_agent_execution_store().await;
    seed_waiting_agent_execution(event_store.as_ref(), "mock-session", "turn-dynamic", action)
        .await;
    let state = test_state_with_event_store_and_agent_execution_store(
        false,
        event_store.clone(),
        event_store,
    )
    .await;
    let app = Router::new()
        .route("/v1/turns/input", post(submit_turn_input_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/turns/input")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "identity": {
                            "sessionId": "mock-session",
                            "turnId": "turn-dynamic"
                        },
                        "input": {
                            "type": "userActionResponse",
                            "actionId": "action-dynamic",
                            "response": {
                                "type": "dynamicToolResult",
                                "response": { "output": "done", "success": true }
                            }
                        },
                        "clientRequestId": "request-action"
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
    assert_eq!(payload["data"]["requestId"], "request-action");
    assert_eq!(payload["data"]["identity"]["turnId"], "turn-dynamic");
    assert_eq!(payload["data"]["messageId"], "msg_action_action-dynamic");
}
