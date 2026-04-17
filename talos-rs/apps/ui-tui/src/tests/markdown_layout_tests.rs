use super::*;

#[test]
fn test_markdown_code_block_uses_distinct_style() {
    let md = "text\n```rust\nlet x = 1;\n```\nafter";
    let lines = render_markdown_lines(md);
    // Line 0: "text", line 1: fence label, line 2: code, line 3: close fence, line 4: "after"
    assert!(lines.len() >= 3);
    // The code line should have LightGreen fg
    let code_line = &lines[2];
    let is_green = code_line
        .spans
        .iter()
        .any(|s| s.style.fg == Some(Color::LightGreen));
    assert!(is_green, "code block lines should be LightGreen");
}

#[test]
fn test_assistant_block_renders_full_markdown_without_role_label() {
    let app = create_embedded_app();
    let block = ChatBlock::new(
        ChatRole::Assistant,
        "## Summary\n\n1. First\n2. Second\n\n```rust\nlet x = 1;\n```".to_string(),
    );

    let lines = app.assistant_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts.first().map(String::as_str), Some("🤖 Assistant"));
    assert_eq!(texts.get(1).map(String::as_str), Some(""));
    assert!(texts.iter().any(|line| line == "  ▌ Summary"));
    assert!(texts.iter().any(|line| line == "  1. First"));
    assert!(texts.iter().any(|line| line == "  2. Second"));
    assert!(texts.iter().any(|line| line == "  ─── rust ───"));
    assert!(texts.iter().any(|line| line == "  let x = 1;"));
    assert_eq!(
        texts
            .iter()
            .filter(|line| line.starts_with("🤖 Assistant"))
            .count(),
        1
    );
}

#[test]
fn test_user_block_inserts_blank_line_after_header() {
    let app = create_embedded_app();
    let block = ChatBlock::new(ChatRole::User, "hello world".to_string());

    let texts = app
        .user_block_lines(&block, 80)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts, vec!["👤 User", "", "  hello world"]);
}

#[test]
fn test_thinking_block_inserts_blank_line_after_header() {
    let app = create_embedded_app();
    let block = ChatBlock::new(ChatRole::Thinking, "step one".to_string());

    let texts = app
        .thinking_block_lines(&block, 80)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts, vec!["💭 Thinking", "", "  step one"]);
}

#[test]
fn test_chat_content_columns_apply_left_padding() {
    let app = create_embedded_app();
    let (chat_rect, scrollbar_rect) = app.chat_content_columns(Rect::new(1, 1, 20, 6));

    assert_eq!(chat_rect, Rect::new(3, 1, 17, 6));
    assert_eq!(scrollbar_rect, Rect::new(20, 1, 1, 6));
}

#[test]
fn test_right_panel_hidden_by_default() {
    let app = create_embedded_app();
    assert!(!app.right_panel_visible);
}

// ── Phase 5: history + model picker ──────────────────────────────────

#[tokio::test]
async fn test_ctrl_h_switches_to_history_state() {
    let mut app = create_remote_app();
    app.initialize_remote().await;
    assert_eq!(app.current_state, AppState::Chat);

    app.open_history();

    assert_eq!(app.current_state, AppState::History);
}

#[tokio::test]
async fn test_ctrl_b_toggles_right_panel() {
    let mut app = create_remote_app();
    assert!(!app.right_panel_visible);

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert!(app.right_panel_visible);

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert!(!app.right_panel_visible);
}

#[test]
fn test_toggle_model_picker_shows_picker() {
    let mut app = create_remote_app();
    app.available_models = vec!["gpt-4o".to_string(), "qwen-plus".to_string()];
    app.current_model = "gpt-4o".to_string();
    assert!(!app.show_model_picker);

    app.toggle_model_picker();
    assert!(app.show_model_picker);
    // Pre-selects current model index
    assert_eq!(app.model_picker_selected, 0);

    // Toggle again closes
    app.toggle_model_picker();
    assert!(!app.show_model_picker);
}

#[test]
fn test_toggle_model_picker_without_models_shows_hint() {
    let mut app = create_remote_app();
    app.available_models.clear();
    app.toggle_model_picker();
    assert!(
        !app.show_model_picker,
        "picker should not open without models"
    );
    assert!(app
        .chat_blocks
        .iter()
        .any(|b| b.content.contains("No models")));
}

#[tokio::test]
async fn test_command_autocomplete_inserts_command_only() {
    let mut app = create_remote_app();

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.input_buffer, "/sessions");
    assert_eq!(app.cursor_index, "/sessions".chars().count());
    assert!(!app.show_command_autocomplete);
}

#[tokio::test]
async fn test_command_autocomplete_syncs_query_into_input_buffer() {
    let mut app = create_remote_app();

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.input_buffer, "/qui");
    assert_eq!(app.cursor_index, "/qui".chars().count());
    assert_eq!(app.command_input_buffer, "qui");
    assert!(app.show_command_autocomplete);
    assert_eq!(app.command_suggestions.len(), 1);
    assert_eq!(app.command_suggestions[0].name, "quit");
}

#[tokio::test]
async fn test_command_autocomplete_backspace_keeps_input_buffer_in_sync() {
    let mut app = create_remote_app();

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE))
        .await
        .unwrap();
    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.input_buffer, "/q");
    assert_eq!(app.cursor_index, "/q".chars().count());
    assert_eq!(app.command_input_buffer, "q");
    assert!(app.show_command_autocomplete);

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.input_buffer, "/");
    assert_eq!(app.cursor_index, 1);
    assert!(app.command_input_buffer.is_empty());
    assert!(!app.show_command_autocomplete);
}

#[test]
fn test_command_autocomplete_panel_height_is_inline_and_capped() {
    let mut app = create_remote_app();
    app.show_command_autocomplete = true;
    app.command_suggestions = CommandSuggestion::all();

    assert_eq!(app.command_autocomplete_panel_height(), 7);

    app.command_suggestions.clear();
    assert_eq!(app.command_autocomplete_panel_height(), 1);
}

#[tokio::test]
async fn test_history_enter_switches_session() {
    let mut app = create_remote_app();
    app.initialize_remote().await;

    let mut config = talos_types::TalosConfig::default();
    config.llm.model = "claude-3".to_string();
    app.history_sessions = vec![TalosSession {
        title: "Fix tui paste".to_string(),
        context: talos_types::SessionContext {
            session_id: "hist-sess-1".to_string(),
            workspace_id: None,
            source_type: talos_types::talos::TalosSourceType::Http,
            provider: talos_types::talos::TalosProvider::Sdk,
            user_id: "u".to_string(),
            message_id: "m".to_string(),
            trace_id: "t".to_string(),
            channel_id: None,
            thread_id: None,
            project_id: None,
            current_directory: None,
            project_root: None,
            environment: None,
        },
        project_id: None,
        current_directory: None,
        config,
        state: talos_types::SessionState {
            created_at: 0,
            last_activity: 0,
            message_count: 3,
            usage: talos_types::talos::TalosUsage::default(),
        },
    }];
    app.current_state = AppState::History;
    app.history_selected = 0;

    app.switch_to_history_session();

    assert_eq!(app.session_id.as_deref(), Some("hist-sess-1"));
    assert_eq!(app.current_model, "claude-3");
    assert_eq!(app.current_state, AppState::Chat);
}
