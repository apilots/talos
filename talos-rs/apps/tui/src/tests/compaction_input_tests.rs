use super::*;

fn interactive_transcript_text(app: &TuiApp) -> String {
    app.render_runtime
        .interactive_projection_lines()
        .into_iter()
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn test_context_compaction_stream_renders_right_panel_status() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(context_compaction_block_started_event(
        "msg-compact",
        "sess-test",
        1,
        ContextCompactionStage::Preparing,
        "Preparing session history compaction.",
    ));
    app.handle_stream_event(context_compaction_delta_event(
        "msg-compact",
        "sess-test",
        2,
        ContextCompactionStage::Completed,
        Some(ContextCompactionOutcome::Applied),
        "Session history compaction finished.",
    ));

    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("Preparing session history compaction.")));
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("Session history compaction finished.")));
    assert!(!interactive_transcript_text(&app).contains("compaction"));
    assert_eq!(app.active_context_compaction_stage(), None);
}

#[tokio::test]
async fn test_context_compaction_rejects_message_and_preserves_draft() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());
    app.handle_stream_event(context_compaction_block_started_event(
        "msg-compact",
        "sess-test",
        1,
        ContextCompactionStage::Summarizing,
        "Summarizing session history.",
    ));
    app.input_buffer = "keep this draft".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    assert!(app
        .input_status_line(120)
        .to_string()
        .contains("Context compacting... summarizing"));

    app.submit_input().await.unwrap();

    assert_eq!(app.input_buffer, "keep this draft");
    assert!(app.message_queue.messages.is_empty());
    assert!(app
        .status_hint
        .as_ref()
        .is_some_and(|(message, _)| message == "Context compacting... Please wait."));
}

#[tokio::test]
async fn test_context_compaction_rejects_session_command() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());
    app.handle_stream_event(context_compaction_block_started_event(
        "msg-compact",
        "sess-test",
        1,
        ContextCompactionStage::RebuildingPrompt,
        "Rebuilding prompt.",
    ));
    let transcript_before = interactive_transcript_text(&app);
    app.input_buffer = "/clear".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    app.submit_input().await.unwrap();

    assert_eq!(app.input_buffer, "/clear");
    assert_eq!(interactive_transcript_text(&app), transcript_before);
}

#[tokio::test]
async fn test_recovery_keeps_running_context_compaction_for_follow_up_delta() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    let response = RecoverTimelineResponse {
        replay: TimelineReplay {
            events: vec![context_compaction_block_started_event(
                "msg-compact",
                "sess-test",
                1,
                ContextCompactionStage::Preparing,
                "Preparing session history compaction.",
            )],
            high_water_mark_event_id: Some("ev_1".to_string()),
            next_cursor: None,
            last_event_seq: 1,
            cursor_status: TimelineCursorStatus::Current,
        },
        active_turn: Some(TimelineActiveTurn {
            identity: AgentTurnRef::new("sess-test", "turn-test"),
            initial_message_id: Some("msg-compact".to_string()),
            state: ActiveTurnState::Running,
            last_event_id: "ev_1".to_string(),
            updated_at: 1,
        }),
    };

    app.replay_recovery_snapshot(response);

    assert!(app.in_flight);
    assert_eq!(
        app.active_context_compaction_stage(),
        Some(ContextCompactionStage::Preparing)
    );
    app.handle_stream_event(context_compaction_delta_event(
        "msg-compact",
        "sess-test",
        2,
        ContextCompactionStage::Summarizing,
        None,
        "Summarizing session history.",
    ));
    assert_eq!(
        app.active_context_compaction_stage(),
        Some(ContextCompactionStage::Summarizing)
    );
}

#[tokio::test]
async fn test_compact_command_subscribes_to_session_timeline_before_request() {
    let client = Arc::new(MockGatewayClient::new());
    {
        let mut state = client.state.lock().unwrap();
        state.timeline_high_water_mark_event_id = Some("ev_41".to_string());
        state.session_subscription_events = vec![
            context_compaction_block_started_event(
                "msg-compact",
                "sess-test",
                42,
                ContextCompactionStage::Preparing,
                "Preparing session history compaction.",
            ),
            context_compaction_delta_event(
                "msg-compact",
                "sess-test",
                43,
                ContextCompactionStage::Summarizing,
                None,
                "Summarizing session history.",
            ),
        ];
    }
    let gateway_client: Arc<dyn GatewayClient> = client.clone();
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());

    app.compact_session_command().unwrap();
    for _ in 0..10 {
        tokio::task::yield_now().await;
        app.poll_stream_connect().await;
        app.poll_stream_events();
        app.poll_command_updates();
        if client.state.lock().unwrap().compact_calls == 1 {
            break;
        }
    }

    let state = client.state.lock().unwrap();
    assert_eq!(state.session_subscription_calls, 1);
    assert_eq!(
        state.session_subscription_since_event_ids,
        vec![Some("ev_41".to_string())]
    );
    assert_eq!(state.compact_calls, 1);
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("Summarizing session history.")));
}

#[tokio::test]
async fn test_compact_command_rejects_input_before_the_first_delta() {
    let client = Arc::new(MockGatewayClient::new());
    let gateway_client: Arc<dyn GatewayClient> = client.clone();
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.input_buffer = "keep this draft".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    app.compact_session_command().unwrap();

    assert!(app
        .input_status_line(120)
        .to_string()
        .contains("Context compacting... preparing"));
    app.submit_input().await.unwrap();
    assert_eq!(app.input_buffer, "keep this draft");
    assert!(app.message_queue.messages.is_empty());

    app.input_buffer = "/clear".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    app.submit_input().await.unwrap();
    assert_eq!(app.input_buffer, "/clear");
    assert_eq!(client.state.lock().unwrap().clear_calls, 0);
}

#[tokio::test]
async fn test_compact_rpc_failure_before_the_first_delta_clears_local_busy_state() {
    let client = Arc::new(MockGatewayClient::new());
    {
        let mut state = client.state.lock().unwrap();
        state.compact_failures_remaining = 1;
        state.timeline_high_water_mark_event_id = Some("ev_9".to_string());
    }
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.input_buffer = "keep this draft".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    app.compact_session_command().unwrap();
    for _ in 0..10 {
        tokio::task::yield_now().await;
        app.poll_command_updates();
        if app.pending_command_update.is_none() {
            break;
        }
    }

    assert!(!app.in_flight);
    assert!(app.pending_stream_connect.is_none());
    assert!(app.pending_stream.is_none());
    assert_eq!(app.input_buffer, "keep this draft");
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line == "session compact failed"));
}
