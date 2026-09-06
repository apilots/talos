use super::*;
use crate::types::CommandSuggestion;
use talos_sdk::ReasoningEffort;

fn interactive_transcript_text(app: &TuiApp) -> String {
    app.render_runtime
        .interactive_projection_lines()
        .into_iter()
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_command_suggestions_expose_sessions_without_history_alias() {
    let commands = CommandSuggestion::all()
        .into_iter()
        .map(|command| command.insert_text)
        .collect::<Vec<_>>();

    assert!(commands.contains(&"/sessions".to_string()));
    assert!(!commands.contains(&"/history".to_string()));
}

#[test]
fn test_model_catalog_uses_explicit_default_instead_of_first_entry() {
    let mut app = create_embedded_app();
    let first = mock_model_info("deepseek-v4-flash", 128_000);
    let default = mock_model_info("qwen3.6-flash", 128_000);

    app.apply_model_catalog(talos_sdk::ModelListResponse {
        models: vec![first, default],
        default_model_selection: talos_sdk::ModelSelection::without_reasoning(
            talos_sdk::ModelRef {
                provider: "mock".to_string(),
                id: "qwen3.6-flash".to_string(),
            },
        ),
    })
    .unwrap();

    assert_eq!(app.current_model, "qwen3.6-flash");
}

#[tokio::test]
async fn test_model_picker_enters_reasoning_step_and_esc_returns_to_model_step() {
    let mut app = create_embedded_app();
    set_test_model_info(&mut app, "gpt-5.5", 128_000);
    app.available_model_info[0].model.reasoning = true;
    app.available_model_info[0].model.reasoning_effort = ReasoningEffort::High;
    app.current_model = "gpt-5.5".to_string();
    app.reasoning_effort_override = Some(ReasoningEffort::High);

    app.toggle_model_picker();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    let mut picker = app.model_picker.clone().expect("model picker should open");
    assert!(!picker.back());

    app.handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    let mut picker = app
        .model_picker
        .clone()
        .expect("model picker should stay open");
    assert!(picker.back());
}

#[tokio::test]
async fn test_model_picker_confirm_writes_command_result_block() {
    let mut app = create_embedded_app();
    set_test_model_info(&mut app, "gpt-5.5", 128_000);
    app.available_model_info[0].model.reasoning = true;
    app.available_model_info[0].model.reasoning_effort = ReasoningEffort::Medium;
    app.current_model = "gpt-5.5".to_string();
    app.reasoning_effort_override = Some(ReasoningEffort::Medium);

    app.toggle_model_picker();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    let transcript = interactive_transcript_text(&app);
    assert!(app.model_picker.is_none());
    assert_eq!(app.reasoning_effort_override, Some(ReasoningEffort::High));
    assert!(transcript.contains("Model changed to gpt-5.5 high"));

    app.toggle_model_picker();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    for _ in 0..3 {
        app.handle_input(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await
            .unwrap();
    }
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.reasoning_effort_override, Some(ReasoningEffort::Light));
    assert_eq!(
        app.current_model_selection().unwrap().reasoning_effort,
        Some(ReasoningEffort::Light)
    );
}

#[test]
fn test_input_cursor_and_backspace() {
    let mut app = create_embedded_app();
    app.insert_char('a');
    app.insert_char('b');
    app.cursor_index = 1;
    app.handle_backspace();
    assert_eq!(app.input_buffer, "b");
    assert_eq!(app.cursor_index, 0);
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
    assert_eq!(rendered, vec!["❯ alpha", "  beta█"]);
}

#[test]
fn test_short_long_single_line_paste_wraps_inside_draft() {
    let mut app = create_embedded_app();
    let pasted = "A".repeat(120);

    app.handle_paste(pasted);

    assert!(app.pasted_blocks.is_empty());
    assert_eq!(app.draft_input_lines(24).len(), 6);
    let rendered = app
        .input_lines(24, 8)
        .into_iter()
        .map(|line| line_plain_text(&line))
        .collect::<Vec<_>>();
    assert!(rendered.len() > 1);
    let expected_first_line = format!("❯ {}", "A".repeat(22));
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
async fn test_tab_reports_inline_tool_details_without_pasted_behavior() {
    let mut app = create_embedded_app();

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app
        .status_hint
        .as_ref()
        .map(|(message, _)| message.contains("Tool details are shown inline"))
        .unwrap_or(false));

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
        .await
        .unwrap();
    assert!(app
        .status_hint
        .as_ref()
        .map(|(message, _)| message.contains("Tool details are shown inline"))
        .unwrap_or(false));
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
    state.lock().unwrap().active_turn_id = Some("turn-test".to_string());

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
async fn test_escape_without_in_flight_clears_pasted_block_selection_only() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.selected_pasted_block = Some(0);

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.selected_pasted_block.is_none());
    assert!(state.lock().unwrap().cancelled_sessions.is_empty());
}

#[test]
fn test_finish_active_stream_clears_live_turn_metrics() {
    let mut app = create_embedded_app();
    app.in_flight = true;
    app.turn_metrics_live = true;

    app.finish_active_stream("stream disconnected", true, false);

    assert!(!app.in_flight);
    assert!(!app.turn_metrics_live);
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
    assert!(interactive_transcript_text(&app).contains(&format!("Explain this log\n\n{}", pasted)));
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
    app.selected_skills = vec![SkillSuggestion {
        name: "web-design-guidelines".to_string(),
        description: "Review web UI against design guidelines.".to_string(),
        short_description: None,
        root: PathBuf::from("/tmp/skills/web-design-guidelines"),
        path: PathBuf::from("/tmp/skills/web-design-guidelines/SKILL.md"),
        scope: talos_sdk::SkillScope::External,
        source_plugin: None,
        source_install: None,
        metadata: None,
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
    assert_eq!(request.input.content.len(), 2);
    assert!(matches!(
        &request.input.content[0],
        AgentUserContent::Text { text } if text == "please use $web-design-guidelines"
    ));
    assert!(matches!(
        &request.input.content[1],
        AgentUserContent::SkillSelection { name, path }
            if name == "web-design-guidelines"
                && path == "/tmp/skills/web-design-guidelines/SKILL.md"
    ));
}

#[tokio::test]
async fn test_submit_input_sends_model_and_directory_after_first_turn() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.current_model = "qwen-plus".to_string();
    app.startup_directory = PathBuf::from("/tmp/talos-tui");
    app.input_buffer = "check current weather".to_string();
    app.cursor_index = app.input_buffer.chars().count();

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
    assert_eq!(
        request.model_selection.as_ref().map(|selection| {
            (
                selection.model_ref.provider.as_str(),
                selection.model_ref.id.as_str(),
            )
        }),
        Some(("mock", "qwen-plus"))
    );
    assert_eq!(
        request.current_directory.as_deref(),
        Some(std::path::Path::new("/tmp/talos-tui"))
    );
}

#[test]
fn test_compose_user_input_text_expands_pasted_blocks_without_markers() {
    let mut app = create_embedded_app();
    let marker = "[Pasted text #1 · 2 lines]".to_string();
    app.input_buffer = format!("Explain this log\n\n{}", marker);
    app.cursor_index = app.input_buffer.chars().count();
    app.pasted_blocks
        .push(PastedTextBlock::new(marker, "line1\nline2".to_string()));

    let composed = app.compose_user_input_text().expect("compose input");

    assert_eq!(composed, "Explain this log\n\nline1\nline2");
}

#[tokio::test]
async fn test_submit_input_restores_pasted_blocks_when_queue_is_full() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    for index in 2..=11 {
        assert!(app.send_user_message(format!("m{index}")).await.unwrap());
    }
    assert_eq!(app.message_queue.messages.len(), 10);

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
    let client = Arc::new(MockGatewayClient::new());
    {
        let mut state = client.state.lock().unwrap();
        state.models[0].model.reasoning = true;
        state.models[0].model.reasoning_effort = ReasoningEffort::Medium;
    }
    let gateway_client: Arc<dyn GatewayClient> = client.clone();
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    assert_eq!(app.session_id.as_deref(), None);
    assert!(app.available_models.iter().any(|m| m == "mock:qwen-plus"));
    assert_eq!(app.connection_status, "connected");
    assert_eq!(app.reasoning_effort_override, Some(ReasoningEffort::Medium));

    client.state.lock().unwrap().models[0]
        .model
        .reasoning_effort = ReasoningEffort::High;
    app.refresh_models_from_gateway().await.unwrap();

    assert_eq!(app.reasoning_effort_override, Some(ReasoningEffort::Medium));
    assert_eq!(
        app.current_model_selection().unwrap().reasoning_effort,
        Some(ReasoningEffort::Medium)
    );
    assert_eq!(app.current_model_status_label(), "qwen-plus medium");
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
    app.input_buffer = "/model mock:gpt-5.3 medium".to_string();
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
    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("/model mock:gpt-5.3 medium"));
    assert!(transcript.contains("Model changed to gpt-5.3 medium"));
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
    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("/compact"));
    assert!(transcript.contains("Session compaction completed"));
}

#[tokio::test]
async fn test_clear_command_resets_local_chat_and_session_usage() {
    let client = Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.session_usage = TokenUsage {
        input_tokens: 20,
        output_tokens: 10,
        total_tokens: 30,
        cache_creation_input_tokens: Some(4),
        cache_read_input_tokens: Some(6),
    };
    app.render_runtime.append_system_notice("old answer");
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
    assert_eq!(app.session_usage, TokenUsage::default());
    let transcript = interactive_transcript_text(&app);
    assert!(!transcript.contains("old answer"));
    assert!(transcript.contains("Session context cleared"));
}

#[tokio::test]
async fn test_help_command_writes_transcript() {
    let mut app = create_embedded_app();
    app.input_buffer = "/help".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();

    assert!(!should_quit);
    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("/help"));
    assert!(transcript.contains("Commands: /help /model /models /permissions /sessions"));
}

#[tokio::test]
async fn test_status_command_displays_cache_usage_metrics() {
    let client = Arc::new(MockGatewayClient::new());
    client.state.lock().unwrap().session_usage = TokenUsage {
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
    app.session_usage = TokenUsage {
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
    let status_text = interactive_transcript_text(&app);
    assert_eq!(
        status_text
            .lines()
            .filter(|line| *line == "/status")
            .count(),
        1
    );
    assert!(status_text.contains("Talos TUI"));
    assert!(
        status_text.contains("Model:            qwen-plus (mock, reasoning off, summaries auto)")
    );
    assert!(status_text.contains("Account:          talos@example.com"));
    assert!(status_text.contains("Session:          sess-test"));
    assert!(status_text.contains("Runtime:          embedded ● connected"));
    assert!(status_text.contains("165 tokens · cache read 21"));
}

#[tokio::test]
async fn test_start_turn_stream_updates_final_answer_block() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    assert!(app.send_user_message("hello".to_string()).await.unwrap());

    for _ in 0..40 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !app.in_flight && app.message_queue.messages.is_empty() {
            break;
        }
    }

    let final_answer = interactive_transcript_text(&app);
    assert!(final_answer.contains("# Title"));
    assert!(final_answer.contains("- item"));
    assert!(final_answer.contains("hello"));
}

#[tokio::test]
async fn test_thinking_block_is_not_created_before_first_delta() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    assert!(app.send_user_message("hello".to_string()).await.unwrap());

    assert!(app.in_flight);
    assert!(!app.received_output_delta);
    assert!(interactive_transcript_text(&app).contains("hello"));
    assert!(app.is_agent_working());
}

#[tokio::test]
async fn test_reasoning_delta_renders_plain_text_and_preserves_paragraphs() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(reasoning_block_started_event(
        "msg-think",
        "sess-test",
        1,
        "First line",
    ));
    app.handle_stream_event(reasoning_block_started_event(
        "msg-think",
        "sess-test",
        2,
        "\n\n",
    ));
    app.handle_stream_event(reasoning_block_started_event(
        "msg-think",
        "sess-test",
        3,
        "Second paragraph",
    ));

    let thinking = interactive_transcript_text(&app);

    assert!(thinking.ends_with("First line\n\nSecond paragraph"));
}

#[test]
fn test_history_replay_feeds_transcript_projection() {
    let mut app = create_embedded_app();
    let snapshot = completed_history_response(
        vec![
            completed_text_message(
                "message-user",
                talos_sdk::AgentMessageRole::User,
                "history question",
            ),
            completed_text_message(
                "message-agent",
                talos_sdk::AgentMessageRole::Agent,
                "history answer",
            ),
        ],
        Some(12_300),
    );

    let item_count = app.replay_history_snapshot(snapshot);

    assert_eq!(item_count, 2);
    let pending_text = app
        .render_runtime
        .store()
        .pending_terminal_projection()
        .into_iter()
        .flat_map(|block| block.lines)
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(pending_text.contains("history answer"));
    app.rebuild_chat_render_state(ratatui::layout::Rect::new(0, 0, 80, 20));
    let rendered_text = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered_text.contains("✻ Brewed for 12s"));
}

#[test]
fn test_history_replay_feeds_input_history() {
    let mut app = create_embedded_app();
    let snapshot = completed_history_response(
        vec![completed_text_message(
            "message-user",
            talos_sdk::AgentMessageRole::User,
            "previous question",
        )],
        None,
    );

    app.replay_history_snapshot(snapshot);
    app.navigate_message_history(-1);

    assert_eq!(app.input_buffer, "previous question");
}

#[test]
fn test_stream_events_feed_transcript_projection() {
    let mut app = create_embedded_app();
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(assistant_block_started_event(
        "msg-transcript",
        "sess-test",
        1,
    ));
    app.handle_stream_event(assistant_text_delta_event(
        "msg-transcript",
        "sess-test",
        2,
        "hello",
    ));
    app.handle_stream_event(assistant_phase_delta_event(
        "msg-transcript",
        "sess-test",
        3,
        talos_sdk::AgentAssistantTextPhase::FinalAnswer,
    ));
    app.handle_stream_event(assistant_block_completed_event(
        "msg-transcript",
        "sess-test",
        4,
        "hello",
    ));

    let pending_text = app
        .render_runtime
        .store()
        .pending_terminal_projection()
        .into_iter()
        .flat_map(|block| block.lines)
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(pending_text.contains("hello"));
}

#[tokio::test]
async fn test_modes_alias_works_as_models() {
    let client = Arc::new(MockGatewayClient::new());
    {
        let mut state = client.state.lock().unwrap();
        state.models[0].model.reasoning = true;
        state.models[0].model.reasoning_effort = ReasoningEffort::Medium;
    }
    let gateway_client: Arc<dyn GatewayClient> = client.clone();
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    app.reasoning_effort_override = Some(ReasoningEffort::High);
    client.state.lock().unwrap().models[0]
        .model
        .reasoning_effort = ReasoningEffort::Light;
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
    assert!(interactive_transcript_text(&app).contains("Available models"));
    assert_eq!(app.reasoning_effort_override, Some(ReasoningEffort::High));
    assert_eq!(
        app.current_model_selection().unwrap().reasoning_effort,
        Some(ReasoningEffort::High)
    );
}

#[tokio::test]
async fn test_queued_messages_do_not_add_system_noise_blocks() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    let before = app.render_runtime.store().blocks().len();
    assert!(app.send_user_message("m2".to_string()).await.unwrap());
    assert_eq!(app.render_runtime.store().blocks().len(), before);
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
    let before = app.render_runtime.store().blocks().len();
    let _ = app.submit_input().await.unwrap();

    assert_eq!(app.render_runtime.store().blocks().len(), before);
    assert_eq!(app.message_queue.messages.len(), 1);
    assert_eq!(
        app.message_queue
            .messages
            .first()
            .map(crate::message_queue::queued_message_text),
        Some("m2".to_string())
    );
    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("m1"));
    assert!(!transcript.contains("m2"));
}

#[tokio::test]
async fn test_submit_input_keeps_text_when_queue_is_full() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    for index in 2..=11 {
        assert!(app.send_user_message(format!("m{index}")).await.unwrap());
    }
    assert_eq!(app.message_queue.messages.len(), 10);

    let before_blocks = app.render_runtime.store().blocks().len();
    app.input_buffer = "overflow".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();
    assert!(!should_quit);
    assert_eq!(app.render_runtime.store().blocks().len(), before_blocks);
    assert_eq!(app.input_buffer, "overflow");
    assert_eq!(app.message_queue.messages.len(), 10);
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

    let before_transcript = interactive_transcript_text(&app);
    app.on_tick().await;

    assert_eq!(interactive_transcript_text(&app), before_transcript);
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("timed out"))
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
async fn test_stale_timeline_events_are_ignored() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.active_stream_turn_id = Some("turn-test".to_string());
    app.last_stream_sequence = Some(3);
    let mut stale_event = assistant_text_delta_event("msg-stale", "sess-test", 4, "stale");
    stale_event.event.identity.turn_id = "turn-stale".to_string();
    app.handle_stream_event(stale_event);

    assert!(!interactive_transcript_text(&app).contains("stale"));
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
    app.active_stream_turn_id = Some("turn-test".to_string());
    app.last_stream_sequence = Some(3);
    app.last_stream_event_id = Some("ev_3".to_string());
    let before = interactive_transcript_text(&app);

    app.handle_stream_event(assistant_text_delta_event(
        "msg-current",
        "sess-test",
        3,
        " duplicate",
    ));

    assert_eq!(interactive_transcript_text(&app), before);
    assert_eq!(app.last_stream_sequence, Some(3));
    assert_eq!(app.last_stream_event_id.as_deref(), Some("ev_3"));
}

#[tokio::test]
async fn test_stale_tool_event_does_not_advance_sequence_or_block_valid_output() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());
    app.active_stream_turn_id = Some("turn-test".to_string());
    app.last_stream_sequence = Some(3);

    let mut stale_event = tool_result_block_completed_event(
        "msg-stale",
        "sess-test",
        99,
        "call-current",
        "bash",
        "stale",
        false,
    );
    stale_event.event.identity.turn_id = "turn-stale".to_string();
    app.handle_stream_event(stale_event);

    assert_eq!(app.last_stream_sequence, Some(3));

    app.handle_stream_event(assistant_block_started_event("msg-current", "sess-test", 4));
    app.handle_stream_event(assistant_text_delta_event(
        "msg-current".to_string(),
        "sess-test",
        5,
        "final answer",
    ));
    app.handle_stream_event(assistant_block_completed_event(
        "msg-current",
        "sess-test",
        6,
        "final answer",
    ));

    assert!(interactive_transcript_text(&app).contains("final answer"));
}

#[tokio::test]
async fn test_tool_result_updates_the_call_linked_by_call_id() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());
    app.render_runtime.append_user_input("Fetch asset size");

    app.handle_stream_event(tool_call_block_started_event(
        "msg-tool-merge",
        "sess-test",
        1,
        "call-start",
        "bash",
        serde_json::json!({"cmd": "curl -sL https://talos.love/assets/index.css | wc -c"}),
    ));

    app.handle_stream_event(tool_result_block_completed_event(
        "msg-tool-merge",
        "sess-test",
        2,
        "call-start",
        "bash",
        "17576",
        false,
    ));

    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("● Bash(curl -sL https://talos.love/assets/index.css | wc -c)"));
    assert!(transcript
        .contains("● Bash(curl -sL https://talos.love/assets/index.css | wc -c) completed"));
    assert!(transcript.contains("17576"));
}

#[tokio::test]
async fn test_tool_progress_updates_transcript_projection() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(tool_call_block_started_event(
        "msg-tool-progress",
        "sess-test",
        1,
        "call-bash",
        "bash",
        serde_json::json!({"cmd": "printf 'first\\nsecond\\n'"}),
    ));
    app.handle_stream_event(tool_progress_event(
        "msg-tool-progress",
        "sess-test",
        2,
        "call-bash",
        talos_sdk::AgentToolProgress {
            kind: talos_sdk::TalosToolProgressKind::Stdout,
            text: "first\n".to_string(),
            occurred_at: 2,
        },
    ));
    app.handle_stream_event(tool_progress_event(
        "msg-tool-progress",
        "sess-test",
        3,
        "call-bash",
        talos_sdk::AgentToolProgress {
            kind: talos_sdk::TalosToolProgressKind::Stdout,
            text: "sec".to_string(),
            occurred_at: 3,
        },
    ));
    app.handle_stream_event(tool_progress_event(
        "msg-tool-progress",
        "sess-test",
        4,
        "call-bash",
        talos_sdk::AgentToolProgress {
            kind: talos_sdk::TalosToolProgressKind::Stdout,
            text: "ond\n".to_string(),
            occurred_at: 4,
        },
    ));

    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("● Bash(printf 'first\\nsecond\\n')"));
    assert!(transcript.contains("  ⎿ second"));
    assert!(!transcript.contains("  ⎿ ond"));
}

#[tokio::test]
async fn test_turn_completed_finishes_stream_without_output_delta() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.received_output_delta = false;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(turn_completed_event("sess-test", 1));

    assert!(!app.in_flight);
    assert!(!app.received_output_delta);
}

#[tokio::test]
async fn test_execution_summary_stays_in_right_panel_not_transcript() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(execution_summary_completed_event(
        "msg-summary",
        "sess-test",
        1,
        "Kernel summary: executed 1 tool call(s).",
    ));

    assert!(!interactive_transcript_text(&app).contains("Kernel summary"));
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("execution summary: Kernel summary: executed 1 tool call(s).")));
}
#[tokio::test]
async fn test_commentary_stream_does_not_update_final_answer_block() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(commentary_block_started_event(
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

    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("I will inspect the workspace."));
    assert!(!app.received_output_delta);
}

#[tokio::test]
async fn test_unknown_stream_moves_to_final_answer_when_committed_final() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(pending_assistant_block_started_event(
        "msg-unknown",
        "sess-test",
        1,
    ));
    app.handle_stream_event(pending_assistant_text_delta_event(
        "msg-unknown",
        "sess-test",
        2,
        "Final answer text.",
    ));

    assert!(interactive_transcript_text(&app).contains("Final answer text."));

    app.handle_stream_event(assistant_phase_delta_event(
        "msg-unknown",
        "sess-test",
        3,
        AgentAssistantTextPhase::FinalAnswer,
    ));

    assert!(interactive_transcript_text(&app).contains("Final answer text."));
    assert!(app.received_output_delta);
}

#[tokio::test]
async fn test_interleaved_unknown_items_do_not_share_live_blocks() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(assistant_block_started_event_with_phase(
        "msg-unknown",
        "sess-test",
        1,
        "unknown-a",
        AgentAssistantTextPhase::Pending,
        "",
    ));
    app.handle_stream_event(block_text_delta_event(
        "msg-unknown",
        "sess-test",
        2,
        "unknown-a",
        "First commentary. ",
    ));
    app.handle_stream_event(assistant_block_started_event_with_phase(
        "msg-unknown",
        "sess-test",
        3,
        "unknown-b",
        AgentAssistantTextPhase::Pending,
        "",
    ));
    app.handle_stream_event(block_text_delta_event(
        "msg-unknown",
        "sess-test",
        4,
        "unknown-b",
        "Final candidate.",
    ));
    app.handle_stream_event(assistant_phase_delta_event_for_block(
        "msg-unknown",
        "sess-test",
        5,
        "unknown-b",
        AgentAssistantTextPhase::FinalAnswer,
    ));

    let transcript = interactive_transcript_text(&app);
    assert!(transcript.contains("First commentary."));
    assert!(transcript.contains("Final candidate."));
    assert!(app.received_output_delta);
}

#[tokio::test]
async fn test_completed_final_content_updates_committed_unknown_final() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(assistant_block_started_event_with_phase(
        "msg-final",
        "sess-test",
        1,
        "unknown-final",
        AgentAssistantTextPhase::Pending,
        "",
    ));
    app.handle_stream_event(block_text_delta_event(
        "msg-final",
        "sess-test",
        2,
        "unknown-final",
        "Partial final",
    ));
    app.handle_stream_event(assistant_phase_delta_event_for_block(
        "msg-final",
        "sess-test",
        3,
        "unknown-final",
        AgentAssistantTextPhase::FinalAnswer,
    ));
    app.handle_stream_event(assistant_block_completed_event_for_block(
        "msg-final",
        "sess-test",
        4,
        "unknown-final",
        "Partial final with canonical suffix.",
    ));

    assert!(interactive_transcript_text(&app).contains("Partial final with canonical suffix."));
}

#[tokio::test]
async fn test_unknown_stream_stays_thinking_when_committed_commentary() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(pending_assistant_block_started_event(
        "msg-unknown",
        "sess-test",
        1,
    ));
    app.handle_stream_event(pending_assistant_text_delta_event(
        "msg-unknown",
        "sess-test",
        2,
        "I will inspect the workspace.",
    ));
    app.handle_stream_event(assistant_phase_delta_event(
        "msg-unknown",
        "sess-test",
        3,
        AgentAssistantTextPhase::Commentary,
    ));

    assert!(interactive_transcript_text(&app).contains("I will inspect the workspace."));
    assert!(!app.received_output_delta);
}

#[tokio::test]
async fn test_tool_using_stream_projects_final_answer_during_tool_activity() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(assistant_block_started_event("msg-tools", "sess-test", 1));
    app.handle_stream_event(assistant_text_delta_event(
        "msg-tools",
        "sess-test",
        2,
        "Draft summary",
    ));
    assert!(interactive_transcript_text(&app).contains("Draft summary"));
    assert!(app.received_output_delta);

    app.handle_stream_event(tool_call_block_started_event(
        "msg-tools",
        "sess-test",
        3,
        "call-buffer",
        "read",
        serde_json::json!({"path":"/tmp/example.txt"}),
    ));
    app.handle_stream_event(assistant_text_delta_event(
        "msg-tools",
        "sess-test",
        4,
        "\nFinal answer",
    ));

    assert!(interactive_transcript_text(&app).contains("Draft summary\nFinal answer"));
    assert!(app.received_output_delta);

    app.handle_stream_event(assistant_block_completed_event(
        "msg-tools",
        "sess-test",
        5,
        "Draft summary\nFinal answer",
    ));

    app.handle_stream_event(turn_completed_event("sess-test", 6));

    assert!(interactive_transcript_text(&app).contains("Draft summary\nFinal answer"));
    assert!(app.received_output_delta);
}

#[tokio::test]
async fn test_completed_final_text_renders_when_no_delta_arrived() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(assistant_block_started_event("msg-final", "sess-test", 1));
    app.handle_stream_event(assistant_block_completed_event(
        "msg-final",
        "sess-test",
        2,
        "Final summary from completed content.",
    ));

    assert!(interactive_transcript_text(&app).contains("Final summary from completed content."));
    assert!(app.received_output_delta);
}

#[tokio::test]
async fn test_stream_event_updates_live_projection() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    app.in_flight = true;
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(assistant_block_started_event("msg-follow", "sess-test", 1));
    app.handle_stream_event(assistant_text_delta_event(
        "msg-follow",
        "sess-test",
        2,
        "hello",
    ));

    assert!(interactive_transcript_text(&app).contains("hello"));
}
