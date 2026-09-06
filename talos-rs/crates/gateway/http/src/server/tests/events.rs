use super::*;
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessageBlockDelta, AgentMessageBlockDeltaContent,
    AgentStopReason, AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
};

fn event_id(event_seq: i64) -> String {
    format!("ev_{event_seq:020}")
}

fn turn_started(turn_id: &str) -> AgentEvent {
    AgentEvent {
        identity: AgentTurnRef::new("mock-session", turn_id),
        sequence: 1,
        occurred_at: 1,
        payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
    }
}

fn text_delta(turn_id: &str, sequence: u64, text: &str) -> AgentEvent {
    AgentEvent {
        identity: AgentTurnRef::new("mock-session", turn_id),
        sequence,
        occurred_at: 2,
        payload: AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
            message_id: format!("message-{turn_id}"),
            block_id: "assistant-1".to_string(),
            delta: AgentMessageBlockDeltaContent::Text(text.to_string()),
        }),
    }
}

fn turn_completed(turn_id: &str, sequence: u64) -> AgentEvent {
    AgentEvent {
        identity: AgentTurnRef::new("mock-session", turn_id),
        sequence,
        occurred_at: 3,
        payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    }
}

#[tokio::test]
async fn test_subscribe_timeline_events_exposes_subscription_closed() {
    let state = test_state(false).await;
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
    assert_sse_headers(&resp);
    let body = axum::body::to_bytes(resp.into_body(), 4096)
        .await
        .expect("event body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains("event: subscription/terminated"));
    assert!(body_text.contains(r#""reason":"completed""#));
}

fn assert_sse_headers(resp: &axum::response::Response) {
    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .expect("SSE response should include content-type");
    assert!(content_type.starts_with("text/event-stream"));
    assert_eq!(
        resp.headers()
            .get(axum::http::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache"),
    );
    assert_eq!(
        resp.headers()
            .get("x-accel-buffering")
            .and_then(|value| value.to_str().ok()),
        Some("no"),
    );
}

#[tokio::test]
async fn test_subscribe_timeline_events_replays_after_cursor() {
    let store = Arc::new(RecordingAgentExecutionStore::default());
    let first = store.append_event(turn_started("turn-1"));
    let second = store.append_event(text_delta("turn-1", 2, "delta after reconnect"));
    let state = test_state_with_event_store(false, store).await;
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
                    event_id(first.event_seq)
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
    assert!(body_text.contains("event: timeline/message/block/delta"));
    assert!(body_text.contains(&event_id(second.event_seq)));
    assert!(body_text.contains("delta after reconnect"));
    assert!(!body_text.contains("event: timeline/turn/started"));
}

#[tokio::test]
async fn test_subscribe_timeline_events_replay_continues_after_historical_terminal() {
    let store = Arc::new(RecordingAgentExecutionStore::default());
    store.append_event(turn_started("turn-1"));
    store.append_event(turn_completed("turn-1", 2));
    store.append_event(turn_started("turn-2"));
    let second = store.append_event(text_delta("turn-2", 2, "second turn output"));
    store.append_event(turn_completed("turn-2", 3));
    let state = test_state_with_event_store(false, store).await;

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

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("event body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains("event: timeline/turn/completed"));
    assert!(body_text.contains(&event_id(second.event_seq)));
    assert!(body_text.contains("second turn output"));
}

#[tokio::test]
async fn test_subscribe_timeline_events_filters_replay_by_turn() {
    let store = Arc::new(RecordingAgentExecutionStore::default());
    store.append_event(turn_started("turn-1"));
    let first_terminal = store.append_event(turn_completed("turn-1", 2));
    store.append_event(turn_started("turn-2"));
    let second = store.append_event(text_delta("turn-2", 2, "target turn output"));
    let second_terminal = store.append_event(turn_completed("turn-2", 3));
    let state = test_state_with_event_store(false, store).await;

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

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("event body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains(&event_id(second.event_seq)));
    assert!(body_text.contains(&event_id(second_terminal.event_seq)));
    assert!(body_text.contains("target turn output"));
    assert!(!body_text.contains(&event_id(first_terminal.event_seq)));
}
