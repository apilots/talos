use super::*;

#[tokio::test]
async fn test_subscribe_session_events_replays_after_cursor() {
    let state = test_state(false);
    let first = state
        .event_service
        .record_talos_event(
            Some("turn-1".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageStarted,
                message_id: "msg-1".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 0,
                payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                    request_id: "req-1".to_string(),
                    model: "qwen-plus".to_string(),
                    usage: TalosUsage::default(),
                    iteration: 0,
                    step_id: "start".to_string(),
                }),
            },
        )
        .await
        .expect("first event should record");
    let second = state
        .event_service
        .record_talos_event(
            Some("turn-1".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: "msg-1".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 1,
                payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                    request_id: "req-1".to_string(),
                    delta: "delta after reconnect".to_string(),
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                }),
            },
        )
        .await
        .expect("second event should record");
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_session_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/sessions/mock-session/events?after_event_id={}",
                    first.event_id
                ))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("event body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains("event: message.output.delta"));
    assert!(body_text.contains(second.event_id.as_str()));
    assert!(body_text.contains("delta after reconnect"));
    assert!(!body_text.contains("event: message.started"));
}

#[tokio::test]
async fn test_subscribe_session_events_replay_continues_after_historical_terminal() {
    let state = test_state(false);
    state
        .event_service
        .record_talos_event(
            Some("turn-1".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageCompleted,
                message_id: "msg-1".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 0,
                payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                    request_id: "req-1".to_string(),
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    total_output_items: 1,
                    iteration: 0,
                    step_id: "done".to_string(),
                    task_count: 0,
                }),
            },
        )
        .await
        .expect("historical terminal event should record");
    let second = state
        .event_service
        .record_talos_event(
            Some("turn-2".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: "msg-2".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 0,
                payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                    request_id: "req-2".to_string(),
                    delta: "second turn output".to_string(),
                    item_index: 0,
                    iteration: 0,
                    step_id: "step-1".to_string(),
                }),
            },
        )
        .await
        .expect("second turn event should record");
    state
        .event_service
        .record_talos_event(
            Some("turn-2".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageCompleted,
                message_id: "msg-2".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 1,
                payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                    request_id: "req-2".to_string(),
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    total_output_items: 1,
                    iteration: 1,
                    step_id: "done".to_string(),
                    task_count: 0,
                }),
            },
        )
        .await
        .expect("second terminal event should record");

    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_session_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/events")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("event body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains("event: message.completed"));
    assert!(body_text.contains(second.event_id.as_str()));
    assert!(body_text.contains("second turn output"));
}
