use super::*;
use tokio_stream::StreamExt;

fn running_turn_store() -> Arc<RecordingAgentExecutionStore> {
    let store = Arc::new(RecordingAgentExecutionStore::default());
    store.set_active_turn(AgentActiveTurnRecord {
        identity: AgentTurnRef::new("mock-session", "turn-1"),
        initial_message_id: Some("msg-1".to_string()),
        state: ActiveTurnState::Running,
        last_event_seq: 1,
        current_action_id: None,
        updated_at: 1,
    });
    store
}

fn add_request(text: &str) -> talos_types::MessageQueueAddRequest {
    talos_types::MessageQueueAddRequest {
        session_id: "mock-session".to_string(),
        input: talos_types::AgentUserInput {
            content: vec![talos_types::AgentUserContent::Text {
                text: text.to_string(),
            }],
        },
    }
}

#[tokio::test]
async fn message_queue_handlers_return_shared_queue_and_typed_full_error() {
    let store = running_turn_store();
    let mut config = test_config();
    config.agent.message_queue.max_messages_per_session = 1;
    let state = build_test_state_with_agent_execution_store(
        TestStateConfig {
            event_store: store.clone(),
            base_config: config,
            auth_disabled: false,
            lifecycle: Arc::new(ServerLifecycle::new()),
            config_store: None,
        },
        store,
    )
    .await;

    let added = add_queued_message_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(add_request("first")),
    )
    .await
    .expect("first message should be queued");
    let queue = expect_success(added.0);
    assert_eq!(queue.revision, 1);
    assert_eq!(queue.max_messages, 1);
    assert_eq!(queue.messages.len(), 1);

    let listed = list_message_queue_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(talos_types::MessageQueueListRequest {
            session_id: "mock-session".to_string(),
        }),
    )
    .await
    .expect("queue should list");
    assert_eq!(expect_success(listed.0), queue);

    let response = add_queued_message_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(add_request("second")),
    )
    .await
    .expect_err("full queue should reject the message")
    .into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error body");
    assert_eq!(
        serde_json::from_slice::<Value>(&body).expect("error json"),
        serde_json::json!({
            "status": "error",
            "error": {
                "code": "messageQueueFull",
                "message": "The session message queue has reached its configured capacity; remove a queued message before adding another.",
                "localization": null
            }
        })
    );
}

#[tokio::test]
async fn session_sse_delivers_live_message_queue_change_without_timeline_id() {
    let state = test_state_with_event_store(false, running_turn_store()).await;
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_timeline_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state.clone());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/events?close_on_terminal=true")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("subscribe request");
    assert_eq!(response.status(), StatusCode::OK);

    let _ = add_queued_message_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(add_request("queued")),
    )
    .await
    .expect("message should queue");

    let mut body = response.into_body().into_data_stream();
    let queue_frame = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let chunk = body
                .next()
                .await
                .expect("SSE stream should remain open")
                .expect("SSE chunk");
            let chunk = String::from_utf8(chunk.to_vec()).expect("SSE text");
            if chunk.contains("event: messageQueue/changed") {
                break chunk;
            }
        }
    })
    .await
    .expect("queue changed notification");
    assert!(queue_frame.contains(r#""sessionId":"mock-session""#));
    assert!(queue_frame.contains(r#""revision":1"#));
    assert!(!queue_frame.contains("id:"));
}
