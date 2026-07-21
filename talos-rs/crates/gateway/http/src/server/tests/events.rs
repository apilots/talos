use super::*;

fn timeline_event(
    turn_id: &str,
    message_id: &str,
    sequence: u64,
    event_type: AgentEventType,
    payload: AgentEventPayload,
) -> AgentEvent {
    AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: None,
        event_type,
        turn_id: turn_id.to_string(),
        message_id: message_id.to_string(),
        session_id: "mock-session".to_string(),
        sequence,
        payload,
    }
}

fn turn_started(turn_id: &str, message_id: &str, sequence: u64) -> AgentEvent {
    timeline_event(
        turn_id,
        message_id,
        sequence,
        AgentEventType::TurnStarted,
        AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
            request_id: format!("req-{turn_id}"),
            turn_id: turn_id.to_string(),
            started_at_ms: 1,
        }),
    )
}

fn text_delta(turn_id: &str, message_id: &str, sequence: u64, text: &str) -> AgentEvent {
    timeline_event(
        turn_id,
        message_id,
        sequence,
        AgentEventType::ItemDelta,
        AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
            request_id: format!("req-{turn_id}"),
            turn_id: turn_id.to_string(),
            item_id: "assistant-1".to_string(),
            delta: TalosTimelineDelta::Text {
                text: text.to_string(),
            },
        }),
    )
}

fn turn_completed(turn_id: &str, message_id: &str, sequence: u64) -> AgentEvent {
    timeline_event(
        turn_id,
        message_id,
        sequence,
        AgentEventType::TurnCompleted,
        AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
            request_id: format!("req-{turn_id}"),
            turn_id: turn_id.to_string(),
            completed_at_ms: 2,
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            metrics: None,
            artifact_seq: None,
        }),
    )
}

#[tokio::test]
async fn test_subscribe_timeline_events_replays_after_cursor() {
    let state = test_state(false).await;
    let first = state
        .event_service
        .record_talos_event(turn_started("turn-1", "msg-1", 0))
        .await
        .expect("first event should record");
    let second = state
        .event_service
        .record_talos_event(text_delta("turn-1", "msg-1", 1, "delta after reconnect"))
        .await
        .expect("second event should record");
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_timeline_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/sessions/mock-session/events?after_event_id={}&close_when_idle=true",
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
    assert!(body_text.contains("event: timeline.item.delta"));
    assert!(body_text.contains(&second.event_id));
    assert!(body_text.contains("delta after reconnect"));
    assert!(!body_text.contains("event: timeline.turn.started"));
}

#[tokio::test]
async fn test_subscribe_timeline_events_replay_continues_after_historical_terminal() {
    let state = test_state(false).await;
    state
        .event_service
        .record_talos_event(turn_completed("turn-1", "msg-1", 0))
        .await
        .expect("historical terminal event should record");
    let second = state
        .event_service
        .record_talos_event(text_delta("turn-2", "msg-2", 0, "second turn output"))
        .await
        .expect("second turn event should record");
    state
        .event_service
        .record_talos_event(turn_completed("turn-2", "msg-2", 1))
        .await
        .expect("second terminal event should record");

    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_timeline_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/events?close_when_idle=true")
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
    assert!(body_text.contains("event: timeline.turn.completed"));
    assert!(body_text.contains(&second.event_id));
    assert!(body_text.contains("second turn output"));
}

#[tokio::test]
async fn test_subscribe_timeline_events_filters_replay_by_turn() {
    let state = test_state(false).await;
    let first_terminal = state
        .event_service
        .record_talos_event(turn_completed("turn-1", "msg-1", 0))
        .await
        .expect("first terminal event should record");
    let second = state
        .event_service
        .record_talos_event(text_delta("turn-2", "msg-2", 0, "target turn output"))
        .await
        .expect("target turn event should record");
    let second_terminal = state
        .event_service
        .record_talos_event(turn_completed("turn-2", "msg-2", 1))
        .await
        .expect("target terminal event should record");

    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_timeline_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/events?turn_id=turn-2&close_when_idle=true")
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
    assert!(body_text.contains(&second.event_id));
    assert!(body_text.contains(&second_terminal.event_id));
    assert!(body_text.contains("target turn output"));
    assert!(!body_text.contains(&first_terminal.event_id));
}
