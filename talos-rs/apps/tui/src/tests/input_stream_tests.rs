use super::*;

#[test]
fn test_input_cursor_and_backspace() {
    let mut app = create_embedded_app();
    app.insert_char('a');
    app.insert_char('b');
    app.cursor_index = 1;
    app.handle_backspace();
    assert_eq!(app.get_input_buffer(), "b");
    assert_eq!(app.input_line_plain(), "❯ █b");
}

#[test]
fn test_handle_paste_stores_long_multiline_text_as_block() {
    let mut app = create_embedded_app();
    let pasted = format!("{}\n{}", "A".repeat(420), "B".repeat(420));

    app.handle_paste(pasted.clone());

    assert_eq!(app.pasted_blocks.len(), 1);
    assert_eq!(app.pasted_blocks[0].text, pasted);
    assert_eq!(app.input_buffer, app.pasted_blocks[0].marker);
}

#[test]
fn test_handle_paste_stores_long_single_line_text_as_block() {
    let mut app = create_embedded_app();
    let pasted = "A".repeat(801);

    app.handle_paste(pasted.clone());

    assert_eq!(app.pasted_blocks.len(), 1);
    assert_eq!(app.pasted_blocks[0].text, pasted);
    assert_eq!(app.input_buffer, app.pasted_blocks[0].marker);
}

#[test]
fn test_handle_paste_inserts_short_multiline_text_into_draft() {
    let mut app = create_embedded_app();

    app.handle_paste("alpha\nbeta".to_string());

    assert_eq!(app.input_buffer, "alpha\nbeta");
    assert_eq!(app.cursor_index, "alpha\nbeta".chars().count());
    assert!(app.pasted_blocks.is_empty());
    let rendered = app
        .input_lines(80, 5)
        .into_iter()
        .map(|line| line_plain_text(&line))
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec!["  ❯ alpha", "    beta█"]);
}

#[test]
fn test_short_long_single_line_paste_wraps_inside_draft() {
    let mut app = create_embedded_app();
    let pasted = "A".repeat(120);

    app.handle_paste(pasted);

    assert!(app.pasted_blocks.is_empty());
    assert_eq!(app.composer_line_count(24), 7);
    let rendered = app
        .input_lines(24, 8)
        .into_iter()
        .map(|line| line_plain_text(&line))
        .collect::<Vec<_>>();
    assert!(rendered.len() > 1);
    let expected_first_line = format!("  ❯ {}", "A".repeat(20));
    assert_eq!(
        rendered.first().map(String::as_str),
        Some(expected_first_line.as_str())
    );
}

#[tokio::test]
async fn test_backspace_removes_last_character_when_input_is_not_empty() {
    let mut app = create_embedded_app();
    app.input_buffer = "ab".to_string();
    app.cursor_index = 2;

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.input_buffer, "a");
    assert_eq!(app.cursor_index, 1);
}

#[tokio::test]
async fn test_tab_toggles_last_tool_block_without_pasted_behavior() {
    let mut app = create_embedded_app();
    let idx = app.push_block(
        ChatRole::Tool,
        "Tool: read\nstatus: completed\nresult: ok".to_string(),
    );
    if let Some(block) = app.chat_blocks.get_mut(idx) {
        block.expanded = false;
    }

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.chat_blocks[idx].expanded);

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
        .await
        .unwrap();
    assert!(!app.chat_blocks[idx].expanded);
}

#[tokio::test]
async fn test_backspace_at_empty_input_removes_last_pasted_block() {
    let mut app = create_embedded_app();
    app.handle_paste(format!("{}\n{}", "A".repeat(420), "B".repeat(420)));
    app.handle_paste(format!("{}\n{}", "X".repeat(420), "Y".repeat(420)));
    app.input_buffer = app.pasted_blocks[0].marker.clone();
    app.cursor_index = app.input_buffer.chars().count();

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.pasted_blocks.len(), 2);
    assert_eq!(
        app.input_buffer,
        app.pasted_blocks[0]
            .marker
            .chars()
            .take(
                app.pasted_blocks[0]
                    .marker
                    .chars()
                    .count()
                    .saturating_sub(1)
            )
            .collect::<String>()
    );
}

#[tokio::test]
async fn test_escape_cancels_in_flight_turn() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.session_id = Some("sess-test".to_string());
    app.in_flight = true;

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

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
async fn test_escape_without_in_flight_clears_local_selection_only() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.chat_selection = Some(ChatSelection {
        anchor: SelectionPoint {
            line_index: 0,
            column: 0,
        },
        focus: SelectionPoint {
            line_index: 0,
            column: 3,
        },
    });
    app.selected_pasted_block = Some(0);

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.chat_selection.is_none());
    assert!(app.selected_pasted_block.is_none());
    assert!(state.lock().unwrap().cancelled_sessions.is_empty());
}

#[tokio::test]
async fn test_submit_input_composes_pasted_blocks_into_plain_text_message() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    let pasted = format!("{}\n{}", "A".repeat(420), "B".repeat(420));
    app.input_buffer = "Explain this log\n\n".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    app.handle_paste(pasted.clone());
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();

    assert!(!should_quit);
    let user_messages = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::User)
        .map(|b| b.content.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        user_messages,
        vec![format!("Explain this log\n\n{}", pasted)]
    );
    assert!(app.pasted_blocks.is_empty());
    assert!(app.input_buffer.is_empty());
}

#[tokio::test]
async fn test_submit_input_sends_structured_skill_selection() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.input_buffer = "please use $web-design-guidelines".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    app.selected_skills = vec![SkillSuggestion {
        name: "web-design-guidelines".to_string(),
        description: "Review web UI against design guidelines.".to_string(),
        short_description: None,
        root: PathBuf::from("/tmp/skills/web-design-guidelines"),
        path: PathBuf::from("/tmp/skills/web-design-guidelines/SKILL.md"),
        scope: talos_sdk::SkillScope::External,
        source_plugin: None,
        source_install: None,
    }];

    let should_quit = app.submit_input().await.unwrap();
    for _ in 0..10 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !state.lock().unwrap().sent_requests.is_empty() {
            break;
        }
    }

    assert!(!should_quit);
    let state = state.lock().unwrap();
    let request = state.sent_requests.last().expect("expected sent request");
    assert_eq!(request.input.len(), 2);
    assert!(matches!(
        &request.input[0],
        TalosInputItem::Text { text } if text == "please use $web-design-guidelines"
    ));
    assert!(matches!(
        &request.input[1],
        TalosInputItem::SkillSelection { name, path }
            if name == "web-design-guidelines"
                && path == "/tmp/skills/web-design-guidelines/SKILL.md"
    ));
}

#[test]
fn test_compose_user_input_text_expands_pasted_blocks_without_markers() {
    let mut app = create_embedded_app();
    let marker = "[Pasted text #1 · 2 lines]".to_string();
    app.input_buffer = format!("Explain this log\n\n{}", marker);
    app.cursor_index = app.input_buffer.chars().count();
    app.pasted_blocks
        .push(PastedTextBlock::new(marker, "line1\nline2".to_string()));

    let composed = app.compose_user_input_text();

    assert_eq!(composed, "Explain this log\n\nline1\nline2");
}

#[tokio::test]
async fn test_submit_input_restores_pasted_blocks_when_queue_is_full() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    assert!(app.send_user_message("m2".to_string()).await.unwrap());
    assert!(app.send_user_message("m3".to_string()).await.unwrap());
    assert!(app.send_user_message("m4".to_string()).await.unwrap());
    assert_eq!(app.message_queue.len(), 3);

    let pasted = format!("{}\n{}", "A".repeat(420), "B".repeat(420));
    app.input_buffer = "overflow\n\n".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    app.handle_paste(pasted.clone());
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();

    assert!(!should_quit);
    assert!(app.input_buffer.starts_with("overflow\n\n[Pasted text #1"));
    assert_eq!(app.pasted_blocks.len(), 1);
    assert_eq!(app.pasted_blocks[0].text, pasted);
}

#[tokio::test]
async fn test_app_server_initialize_loads_models_without_session() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    assert_eq!(app.session_id.as_deref(), None);
    assert!(app.available_models.iter().any(|m| m == "qwen-plus"));
    assert_eq!(app.connection_status, "connected");
}

#[tokio::test]
async fn test_app_server_initialize_retries_model_loading_before_succeeding() {
    let mut app = create_app_server_app_with_model_list_failures(1);

    app.initialize_session().await;

    assert_eq!(app.session_id.as_deref(), None);
    assert_eq!(app.connection_status, "connected");
    assert!(app
        .right_panel_events
        .iter()
        .any(|event| event.contains("model load retry 2/5")));
}

#[tokio::test]
async fn test_app_server_initialize_does_not_create_session() {
    let mut app = create_app_server_app();

    app.initialize_session().await;

    assert_eq!(app.session_id.as_deref(), None);
    assert_eq!(app.connection_status, "connected");
    assert!(app
        .right_panel_events
        .iter()
        .any(|event| event.contains("draft chat ready")));
}

#[tokio::test]
async fn test_model_command_updates_app_server_session_model() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.input_buffer = "/model gpt-5.3 medium".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let should_quit = app.submit_input().await.unwrap();
    for _ in 0..10 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if app.pending_command_update.is_none() {
            break;
        }
    }
    assert!(!should_quit);
    assert_eq!(app.current_model, "gpt-5.3 medium");
}

#[tokio::test]
async fn test_compact_command_triggers_session_compaction() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.input_buffer = "/compact".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();
    for _ in 0..10 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if app.pending_command_update.is_none() {
            break;
        }
    }

    assert!(!should_quit);
    assert_eq!(state.lock().unwrap().compact_calls, 1);
    assert!(app
        .chat_blocks
        .iter()
        .any(|block| block.content.contains("Session compaction completed")));
}

#[tokio::test]
async fn test_clear_command_resets_local_chat_and_session_usage() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.session_usage = TalosUsage {
        input_tokens: 20,
        output_tokens: 10,
        total_tokens: 30,
        cache_creation_input_tokens: Some(4),
        cache_read_input_tokens: Some(6),
    };
    app.push_block(ChatRole::Assistant, "old answer".to_string());
    app.input_buffer = "/clear".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();
    for _ in 0..10 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if app.pending_command_update.is_none() {
            break;
        }
    }

    assert!(!should_quit);
    assert_eq!(state.lock().unwrap().clear_calls, 1);
    assert_eq!(app.session_usage, TalosUsage::default());
    assert!(!app
        .chat_blocks
        .iter()
        .any(|block| block.content == "old answer"));
    assert!(app
        .chat_blocks
        .iter()
        .any(|block| block.content.contains("Session context cleared")));
}

#[tokio::test]
async fn test_status_command_displays_cache_usage_metrics() {
    let client = Arc::new(MockGatewayClient::new());
    client.state.lock().unwrap().session_usage = TalosUsage {
        input_tokens: 120,
        output_tokens: 45,
        total_tokens: 165,
        cache_creation_input_tokens: Some(9),
        cache_read_input_tokens: Some(21),
    };
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.session_usage = TalosUsage {
        input_tokens: 12,
        output_tokens: 5,
        total_tokens: 17,
        cache_creation_input_tokens: Some(2),
        cache_read_input_tokens: Some(3),
    };
    app.input_buffer = "/status".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();
    for _ in 0..10 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if app.pending_command_update.is_none() {
            break;
        }
    }

    assert!(!should_quit);
    let status_text = app
        .chat_blocks
        .iter()
        .filter(|block| block.role == ChatRole::System)
        .map(|block| block.content.as_str())
        .find(|content| content.contains("local.usage.input: 12"))
        .unwrap_or_default()
        .to_string();
    assert!(status_text.contains("local.usage.cache_create: 2"));
    assert!(status_text.contains("local.usage.cache_read: 3"));
    assert!(status_text.contains("server.usage.input: 120"));
    assert!(status_text.contains("server.usage.cache_create: 9"));
    assert!(status_text.contains("server.usage.cache_read: 21"));
}

#[tokio::test]
async fn test_start_turn_stream_updates_assistant_block() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    assert!(app.send_user_message("hello".to_string()).await.unwrap());

    for _ in 0..40 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !app.in_flight && app.message_queue.is_empty() {
            break;
        }
    }

    let assistant = app
        .chat_blocks
        .iter()
        .find(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .unwrap_or_default();
    assert!(assistant.contains("# Title"));
    assert!(assistant.contains("- item"));
    let user_messages = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::User)
        .map(|b| b.content.clone())
        .collect::<Vec<_>>();
    assert_eq!(user_messages, vec!["hello".to_string()]);
}

#[tokio::test]
async fn test_thinking_block_is_not_created_before_first_delta() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    assert!(app.send_user_message("hello".to_string()).await.unwrap());

    assert!(app.in_flight);
    assert!(!app.received_output_delta);
    assert!(!app.chat_blocks.iter().any(|b| b.role == ChatRole::Thinking));
    assert!(!app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Assistant));
    assert!(app.transcript.live_assistant_idx.is_none());
    assert!(app.is_agent_working());
}

#[tokio::test]
async fn test_reasoning_delta_renders_plain_text_and_preserves_paragraphs() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(reasoning_delta_event(
        "msg-think",
        "sess-test",
        1,
        "First line",
    ));
    app.handle_stream_event(reasoning_delta_event("msg-think", "sess-test", 2, "\n\n"));
    app.handle_stream_event(reasoning_delta_event(
        "msg-think",
        "sess-test",
        3,
        "Second paragraph",
    ));

    let thinking = app
        .chat_blocks
        .iter()
        .find(|b| b.role == ChatRole::Thinking)
        .map(|b| b.content.clone())
        .unwrap_or_default();

    assert_eq!(thinking, "First line\n\nSecond paragraph");
}

#[tokio::test]
async fn test_modes_alias_works_as_models() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.input_buffer = "/modes".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let _ = app.submit_input().await.unwrap();
    for _ in 0..10 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if app.pending_command_update.is_none() {
            break;
        }
    }
    assert!(app
        .chat_history
        .iter()
        .any(|line| line.contains("Available models")));
}

#[tokio::test]
async fn test_queue_limit_and_serial_dispatch() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    assert!(app.send_user_message("m2".to_string()).await.unwrap());
    assert!(app.send_user_message("m3".to_string()).await.unwrap());
    assert!(app.send_user_message("m4".to_string()).await.unwrap());
    assert!(!app.send_user_message("m5".to_string()).await.unwrap());

    assert_eq!(app.message_queue.len(), 3);
    assert!(app.in_flight);

    for _ in 0..120 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !app.in_flight && app.message_queue.is_empty() {
            break;
        }
    }

    assert!(app.message_queue.is_empty());
    assert!(!app.in_flight);
    let user_messages = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::User)
        .map(|b| b.content.clone())
        .collect::<Vec<_>>();
    assert_eq!(user_messages, vec!["m1", "m2", "m3", "m4"]);
}

#[tokio::test]
async fn test_queued_messages_do_not_add_system_noise_blocks() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    let before = app.chat_blocks.len();
    assert!(app.send_user_message("m2".to_string()).await.unwrap());
    assert_eq!(app.chat_blocks.len(), before);
}

#[tokio::test]
async fn test_submit_input_queues_without_adding_user_block() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    app.input_buffer = "m1".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let _ = app.submit_input().await.unwrap();

    app.input_buffer = "m2".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let before = app.chat_blocks.len();
    let _ = app.submit_input().await.unwrap();

    assert_eq!(app.chat_blocks.len(), before);
    assert_eq!(app.message_queue.len(), 1);
    assert_eq!(
        app.message_queue.front().map(|msg| msg.text.as_str()),
        Some("m2")
    );
    let user_messages = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::User)
        .map(|b| b.content.as_str())
        .collect::<Vec<_>>();
    assert_eq!(user_messages, vec!["m1"]);
}

#[tokio::test]
async fn test_submit_input_keeps_text_when_queue_is_full() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    assert!(app.send_user_message("m2".to_string()).await.unwrap());
    assert!(app.send_user_message("m3".to_string()).await.unwrap());
    assert!(app.send_user_message("m4".to_string()).await.unwrap());
    assert_eq!(app.message_queue.len(), 3);

    let before_blocks = app.chat_blocks.len();
    app.input_buffer = "overflow".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();
    assert!(!should_quit);
    assert_eq!(app.chat_blocks.len(), before_blocks);
    assert_eq!(app.input_buffer, "overflow");
    assert_eq!(app.message_queue.len(), 3);
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("Queue is full"))
        .unwrap_or(false));
}

#[tokio::test]
async fn test_timeout_uses_status_hint_not_chat_block() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.active_message_raw = Some("slow request".to_string());
    app.active_retry_count = 1;
    app.last_stream_event_at =
        Some(Instant::now() - STREAM_STALL_TIMEOUT - Duration::from_millis(1));

    let before_blocks = app.chat_blocks.len();
    app.on_tick().await;

    assert_eq!(app.chat_blocks.len(), before_blocks);
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("timed out"))
        .unwrap_or(false));
}

#[tokio::test]
async fn test_timeout_reconnects_active_stream_instead_of_retrying_turn() {
    let events = vec![
        started_event("msg-recovered", 0),
        output_item_started_event("msg-recovered", "sess-test", 1),
        output_delta_event("msg-recovered", "sess-test", 2, "recovered reply"),
        completed_event(
            "msg-recovered",
            "sess-test",
            3,
            TalosUsage {
                total_tokens: 25,
                ..TalosUsage::default()
            },
        ),
    ];
    let client = Arc::new(MockGatewayClient::with_active_reconnect_events(events));
    let mut app = TuiApp::new_with_gateway_client(client.clone());
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.in_flight = true;
    app.active_message_raw = Some("slow request".to_string());
    app.active_user_idx = Some(app.push_block(ChatRole::User, "slow request".to_string()));
    app.last_stream_event_id = Some("ev_2".to_string());
    app.last_stream_event_at =
        Some(Instant::now() - STREAM_STALL_TIMEOUT - Duration::from_millis(1));

    app.on_tick().await;
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
    assert!(app.chat_blocks.iter().any(
        |block| block.role == ChatRole::Assistant && block.content.contains("recovered reply")
    ));
    assert!(!app
        .chat_blocks
        .iter()
        .any(|block| block.content.contains("Failed to start stream")));
}

#[tokio::test]
async fn test_stale_timeline_events_are_ignored() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.active_stream_message_id = Some("msg-current".to_string());
    app.last_stream_sequence = Some(3);
    app.transcript.live_assistant_idx = Some(app.push_block(ChatRole::Assistant, String::new()));

    let stale_message_id = format!(
        "{}-stale",
        app.active_stream_message_id
            .as_deref()
            .expect("active stream message id should exist")
    );
    app.handle_stream_event(output_delta_event(
        stale_message_id,
        "sess-test",
        4,
        "stale",
    ));

    let assistant = app
        .chat_blocks
        .iter()
        .find(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .unwrap_or_default();
    assert!(assistant.is_empty());
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("ignored stale event")));
}

#[tokio::test]
async fn test_duplicate_stream_sequence_is_ignored() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());
    app.active_stream_message_id = Some("msg-current".to_string());
    app.last_stream_sequence = Some(3);
    app.last_stream_event_id = Some("ev_3".to_string());
    app.transcript.live_assistant_idx = Some(app.push_block(ChatRole::Assistant, "hello".into()));

    app.handle_stream_event(output_delta_event(
        "msg-current",
        "sess-test",
        3,
        " duplicate",
    ));

    let assistant = app
        .chat_blocks
        .iter()
        .find(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .unwrap_or_default();
    assert_eq!(assistant, "hello");
    assert_eq!(app.last_stream_sequence, Some(3));
    assert_eq!(app.last_stream_event_id.as_deref(), Some("ev_3"));
}

#[tokio::test]
async fn test_stale_tool_event_does_not_advance_sequence_or_block_valid_output() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());
    app.active_stream_message_id = Some("msg-current".to_string());
    app.last_stream_sequence = Some(3);

    app.push_tool_call_block(
        "call-current".to_string(),
        "bash".to_string(),
        serde_json::json!({"cmd": "pwd"}),
        1,
        0,
        None,
    );

    app.handle_stream_event(tool_completed_event(
        "msg-stale",
        "sess-test",
        99,
        "call-current",
        "bash",
        "stale",
        false,
    ));

    assert_eq!(app.last_stream_sequence, Some(3));

    app.handle_stream_event(output_item_started_event("msg-current", "sess-test", 4));
    app.handle_stream_event(output_delta_event(
        "msg-current".to_string(),
        "sess-test",
        5,
        "final answer",
    ));

    let assistant = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .collect::<String>();
    assert!(assistant.contains("final answer"));
}

#[tokio::test]
async fn test_turn_completed_finishes_stream_without_output_delta() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.received_output_delta = false;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(completed_event(
        "msg-new",
        "sess-test",
        1,
        TalosUsage::default(),
    ));

    assert!(!app.in_flight);
    assert!(!app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Assistant));
}

#[tokio::test]
async fn test_execution_summary_stays_in_right_panel_not_transcript() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(execution_summary_event(
        "msg-summary",
        "sess-test",
        1,
        "Kernel summary: executed 1 tool call(s).",
    ));

    assert!(!app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::System && b.content.contains("Execution summary")));
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("execution summary: Kernel summary: executed 1 tool call(s).")));
}

#[tokio::test]
async fn test_commentary_stream_does_not_update_assistant_block() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(commentary_item_started_event(
        "msg-commentary",
        "sess-test",
        1,
    ));
    app.handle_stream_event(commentary_delta_event(
        "msg-commentary",
        "sess-test",
        2,
        "I will inspect the workspace.",
    ));

    assert!(!app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Assistant
            && b.content.contains("I will inspect the workspace.")));
    assert!(app.chat_blocks.iter().any(
        |b| b.role == ChatRole::Thinking && b.content.contains("I will inspect the workspace.")
    ));
    assert!(!app.received_output_delta);
}

#[tokio::test]
async fn test_unknown_stream_moves_to_assistant_when_committed_final() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(unknown_item_started_event("msg-unknown", "sess-test", 1));
    app.handle_stream_event(unknown_delta_event(
        "msg-unknown",
        "sess-test",
        2,
        "Final answer text.",
    ));

    assert!(app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Thinking && b.content.contains("Final answer text.")));
    assert!(!app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Assistant && b.content.contains("Final answer text.")));

    app.handle_stream_event(assistant_text_committed_event(
        "msg-unknown",
        "sess-test",
        3,
        TalosTimelinePhase::FinalAnswer,
    ));

    assert!(!app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Thinking && b.content.contains("Final answer text.")));
    assert!(app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Assistant && b.content.contains("Final answer text.")));
    assert!(app.received_output_delta);
}

#[tokio::test]
async fn test_unknown_stream_stays_thinking_when_committed_commentary() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(unknown_item_started_event("msg-unknown", "sess-test", 1));
    app.handle_stream_event(unknown_delta_event(
        "msg-unknown",
        "sess-test",
        2,
        "I will inspect the workspace.",
    ));
    app.handle_stream_event(assistant_text_committed_event(
        "msg-unknown",
        "sess-test",
        3,
        TalosTimelinePhase::Commentary,
    ));

    assert!(app.chat_blocks.iter().any(
        |b| b.role == ChatRole::Thinking && b.content.contains("I will inspect the workspace.")
    ));
    assert!(!app
        .chat_blocks
        .iter()
        .any(|b| b.role == ChatRole::Assistant
            && b.content.contains("I will inspect the workspace.")));
    assert!(!app.received_output_delta);
}

#[tokio::test]
async fn test_tool_using_stream_keeps_assistant_output_interleaved() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(output_item_started_event("msg-tools", "sess-test", 1));
    app.handle_stream_event(output_delta_event(
        "msg-tools",
        "sess-test",
        2,
        "Draft summary",
    ));
    let assistant_before_tool = app
        .chat_blocks
        .iter()
        .find(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .unwrap_or_default();
    assert_eq!(assistant_before_tool, "Draft summary");

    app.handle_stream_event(tool_started_event(
        "msg-tools",
        "sess-test",
        3,
        "call-buffer",
        "read",
        serde_json::json!({"path":"/tmp/example.txt"}),
    ));
    app.handle_stream_event(output_delta_event(
        "msg-tools",
        "sess-test",
        4,
        "\nFinal answer",
    ));

    let assistant_during_tool = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        assistant_during_tool,
        vec!["Draft summary".to_string(), "\nFinal answer".to_string()]
    );

    app.handle_stream_event(completed_event(
        "msg-tools",
        "sess-test",
        5,
        TalosUsage::default(),
    ));

    let assistant_after_complete = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        assistant_after_complete,
        vec!["Draft summary".to_string(), "\nFinal answer".to_string()]
    );
}

#[tokio::test]
async fn test_stream_event_preserves_manual_scroll_mode() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.chat_manual_scroll = true;
    app.chat_scroll = 5;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(output_item_started_event("msg-follow", "sess-test", 1));
    app.handle_stream_event(output_delta_event("msg-follow", "sess-test", 2, "hello"));

    assert!(app.chat_manual_scroll);
    assert_eq!(app.chat_scroll, 5);
}
