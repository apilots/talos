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

#[tokio::test]
async fn escape_during_start_interrupts_the_acknowledged_turn() {
    let client = Arc::new(MockGatewayClient::new());
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    client.state.lock().unwrap().start_gate = Some((entered.clone(), release.clone()));
    let mut app = TuiApp::new_with_gateway_client(client.clone());
    app.input_buffer = "start a turn".to_string();
    app.cursor_index = app.input_buffer.len();

    tokio::time::timeout(Duration::from_secs(2), async {
        app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        entered.notified().await;
        let session_id = app.session_id.clone().unwrap();
        assert!(client.get_active_turn(&session_id).await.unwrap().is_none());

        for key in [KeyCode::Esc, KeyCode::Esc, KeyCode::Char('x')] {
            app.handle_input(KeyEvent::new(key, KeyModifiers::NONE))
                .await
                .unwrap();
        }
        app.on_tick().await;
        assert_eq!(app.input_buffer, "x");
        assert!(client.state.lock().unwrap().cancelled_sessions.is_empty());

        release.notify_one();
        while app.pending_stream_connect.is_some() || app.pending_interrupt.is_some() {
            app.on_tick().await;
            tokio::task::yield_now().await;
        }

        let state = client.state.lock().unwrap();
        assert_eq!(state.sent_requests.len(), 1);
        assert_eq!(state.cancelled_sessions, vec![session_id]);
        assert_eq!(state.last_cancelled_turn_id.as_deref(), Some("turn-test"));
    })
    .await
    .expect("Esc reaches the Turn after start acknowledgement");
}
