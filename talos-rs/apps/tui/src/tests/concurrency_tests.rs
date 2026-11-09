use super::*;

#[tokio::test]
async fn test_escape_cancels_in_flight_turn() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.session_id = Some("sess-test".to_string());
    app.in_flight = true;
    state.lock().unwrap().active_turn_id = Some("turn-test".to_string());

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        while app.pending_interrupt.is_some() {
            app.poll_interrupt().await;
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("interrupt settled");

    assert_eq!(
        state.lock().unwrap().cancelled_sessions,
        vec!["sess-test".to_string()]
    );
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("Cancelling current turn"))
        .unwrap_or(false));
}

#[tokio::test]
async fn test_timeout_reconnects_active_stream_instead_of_retrying_turn() {
    let events = vec![
        turn_started_event("sess-test", 0),
        assistant_block_started_event("msg-recovered", "sess-test", 1),
        assistant_text_delta_event("msg-recovered", "sess-test", 2, "recovered reply"),
        assistant_block_completed_event("msg-recovered", "sess-test", 3, "recovered reply"),
        metrics_updated_event(
            "sess-test",
            4,
            talos_sdk::AgentRunMetrics {
                usage: TokenUsage {
                    total_tokens: 25,
                    ..TokenUsage::default()
                },
                ..talos_sdk::AgentRunMetrics::default()
            },
        ),
        turn_completed_event("sess-test", 5),
    ];
    let client = Arc::new(MockGatewayClient::with_active_reconnect_events(events));
    let mut app = TuiApp::new_with_gateway_client(client.clone());
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.in_flight = true;
    app.active_message_raw = Some("slow request".to_string());
    app.last_stream_event_id = Some("ev_2".to_string());
    app.last_stream_event_at =
        Some(Instant::now() - STREAM_STALL_TIMEOUT - Duration::from_millis(1));

    app.on_tick().await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while app.pending_reconnect.is_some() {
            tokio::task::yield_now().await;
            app.on_tick().await;
        }
    })
    .await
    .expect("reconnect settled");
    assert_eq!(
        client
            .state
            .lock()
            .unwrap()
            .reconnect_since_event_ids
            .as_slice(),
        &[Some("ev_2".to_string())]
    );
    assert!(app.in_flight);
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("Reconnected"))
        .unwrap_or(false));

    app.on_tick().await;

    assert!(!app.in_flight);
    assert!(interactive_transcript_text(&app).contains("recovered reply"));
    assert!(!interactive_transcript_text(&app).contains("Failed to start stream"));
}
