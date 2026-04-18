use super::*;

#[test]
fn test_request_follow_bottom_updates_scroll_only_when_auto_following() {
    let mut app = create_embedded_app();
    app.chat_manual_scroll = false;
    app.chat_scroll = 3;

    app.request_follow_bottom();
    assert_eq!(app.chat_scroll, u16::MAX);

    app.chat_manual_scroll = true;
    app.chat_scroll = 7;
    app.request_follow_bottom();
    assert_eq!(app.chat_scroll, 7);
}

#[test]
fn test_input_status_line_uses_dynamic_token_left_percent() {
    let mut app = create_embedded_app();
    set_test_model_info(&mut app, "qwen-max", 32_768);
    app.current_model = "qwen-max".to_string();
    app.session_usage = TalosUsage {
        input_tokens: 964,
        output_tokens: 16,
        total_tokens: 980,
        ..TalosUsage::default()
    };

    let text = line_plain_text(&app.input_status_line(120));

    assert!(text.contains("97% left"));
    assert!(!text.contains("48% left"));
}

#[tokio::test]
async fn test_message_completed_updates_session_usage() {
    let mut app = create_remote_app();
    app.initialize_remote().await;
    set_test_model_info(&mut app, "qwen-max", 32_768);
    app.current_model = "qwen-max".to_string();
    app.in_flight = true;
    app.received_output_delta = false;
    app.transcript.live_assistant_idx = Some(app.push_block(ChatRole::Assistant, String::new()));
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(completed_event(
        "msg-usage",
        "sess-test",
        1,
        TalosUsage {
            input_tokens: 964,
            output_tokens: 16,
            total_tokens: 980,
            cache_creation_input_tokens: Some(7),
            cache_read_input_tokens: Some(13),
        },
    ));

    assert_eq!(app.session_usage.total_tokens, 980);
    assert_eq!(app.session_usage.cache_creation_input_tokens, Some(7));
    assert_eq!(app.session_usage.cache_read_input_tokens, Some(13));
    assert_eq!(app.token_left_percent(), Some(97));
}

#[test]
fn test_chat_card_wrapped_lines_keep_indent() {
    let app = create_embedded_app();
    let block = ChatBlock::new(
        ChatRole::Assistant,
        "这是一个非常长的回答，用于测试换行后是否仍然带有缩进。".to_string(),
    );
    let lines = app.chat_card_lines(&block, 12);
    assert!(!lines.is_empty());
    assert!(line_plain_text(&lines[0].display).starts_with("🤖 Assistant"));
    assert!(lines.len() > 1);
}

#[test]
fn test_chat_card_wrapped_lines_never_exceed_visual_width() {
    let app = create_embedded_app();
    let body_width = 10usize;
    let block = ChatBlock::new(
        ChatRole::Assistant,
        "这是一段很长很长的内容，用于验证每一行都不会超过内容窗口宽度并触发终端二次换行。"
            .to_string(),
    );
    let lines = app.chat_card_lines(&block, body_width);

    let max_line_width = body_width + 2;
    for line in lines.iter() {
        let width = line_display_width(&line.display);
        assert!(
            width <= max_line_width,
            "line width {} exceeds max {}",
            width,
            max_line_width
        );
    }
}

#[test]
fn test_selected_chat_text_excludes_border_glyphs() {
    let mut app = create_embedded_app();
    app.chat_blocks = vec![ChatBlock::new(
        ChatRole::Assistant,
        "Alpha Beta".to_string(),
    )];
    app.rebuild_chat_render_state(Rect::new(0, 0, 20, 8));
    app.chat_selection = Some(ChatSelection {
        anchor: SelectionPoint {
            line_index: 2,
            column: 0,
        },
        focus: SelectionPoint {
            line_index: 2,
            column: 10,
        },
    });

    assert_eq!(app.selected_chat_text().as_deref(), Some("Alpha Bet"));
}

#[test]
fn test_selected_chat_text_across_wrapped_lines_preserves_newlines() {
    let mut app = create_embedded_app();
    app.chat_blocks = vec![ChatBlock::new(
        ChatRole::Assistant,
        "abcdefghij".to_string(),
    )];
    app.rebuild_chat_render_state(Rect::new(0, 0, 8, 8));
    // Line 0 is the assistant header, line 1 is the header gap, then body keeps
    // an extra left indent and wraps to ["abcd", "efgh", "ij"].
    app.chat_selection = Some(ChatSelection {
        anchor: SelectionPoint {
            line_index: 2,
            column: 2,
        },
        focus: SelectionPoint {
            line_index: 3,
            column: 6,
        },
    });

    assert_eq!(app.selected_chat_text().as_deref(), Some("abcd\nefgh"));
}

#[test]
#[ignore = "mouse coordinate selection is layout-sensitive and covered separately"]
fn test_mouse_drag_selection_stays_inside_chat_rect() {
    let mut app = create_embedded_app();
    app.chat_blocks = vec![ChatBlock::new(ChatRole::Assistant, "Alpha".to_string())];
    app.rebuild_chat_render_state(Rect::new(4, 3, 20, 8));
    let alpha_line_index = app
        .rendered_chat_lines
        .iter()
        .position(|line| line_plain_text(&line.display) == "Alpha")
        .expect("assistant content line should exist");
    let alpha_row = 3 + alpha_line_index as u16;
    assert!(app
        .selection_point_from_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        })
        .is_none());

    assert_eq!(
        app.selection_point_from_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: alpha_row,
            modifiers: KeyModifiers::empty(),
        }),
        Some(SelectionPoint {
            line_index: alpha_line_index,
            column: 2,
        })
    );
}

#[test]
fn test_round_text_for_block_includes_user_assistant_and_tool_blocks() {
    let mut app = create_embedded_app();
    let mut tool_block = ChatBlock::new_tool_call(
        "call-1".to_string(),
        "read".to_string(),
        json!({"file_path":"/tmp/demo.py"}),
        1,
        0,
        None,
    );
    tool_block.content = "L1: print('hi')".to_string();
    tool_block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    app.chat_blocks = vec![
        ChatBlock::new(ChatRole::User, "Analyze /tmp/demo".to_string()),
        ChatBlock::new(
            ChatRole::Assistant,
            "Planning to inspect the file.".to_string(),
        ),
        tool_block,
        ChatBlock::new(ChatRole::Assistant, "Here is the summary.".to_string()),
    ];

    let text = app
        .round_text_for_block(2)
        .expect("round text should exist for tool block");

    assert!(text.contains("## User\n\nAnalyze /tmp/demo"));
    assert!(text.contains("## Assistant\n\nPlanning to inspect the file."));
    assert!(text.contains("### Tool `Read`"));
    assert!(text.contains("\"file_path\": \"/tmp/demo.py\""));
    assert!(text.contains("```json"));
    assert!(text.contains("Output:\n```text\nL1: print('hi')\n```"));
    assert!(text.contains("## Assistant\n\nHere is the summary."));
}

#[test]
fn test_rebuild_chat_render_state_tracks_round_footer_button() {
    let mut app = create_embedded_app();
    app.chat_blocks = vec![
        ChatBlock::new(ChatRole::User, "First".to_string()),
        ChatBlock::new(ChatRole::Assistant, "Second".to_string()),
    ];
    app.chat_blocks[0].round_timing_ms = Some(1250);
    app.rebuild_chat_render_state(Rect::new(0, 0, 40, 12));

    assert_eq!(app.rendered_round_footer_buttons.len(), 1);

    let (line_index, button) = app
        .rendered_round_footer_buttons
        .iter()
        .next()
        .map(|(line_index, button)| (*line_index, *button))
        .expect("footer button should exist");
    let footer_text = line_plain_text(&app.rendered_chat_lines[line_index].display);
    assert!(footer_text.contains("✻ Brewed for 1s"));
    assert!(footer_text.contains(COPY_ROUND_BUTTON_TEXT));
    assert_eq!(button.round_start_block_index, 0);
}

#[test]
fn test_rebuild_chat_render_state_inserts_spacing_between_blocks() {
    let mut app = create_embedded_app();
    app.chat_blocks = vec![
        ChatBlock::new(ChatRole::System, "Execution summary".to_string()),
        ChatBlock::new(ChatRole::Assistant, "Final answer".to_string()),
    ];

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts[0], "⚙  System");
    assert_eq!(texts[1], "");
    assert_eq!(texts[2], "Execution summary");
    assert_eq!(texts[3], "");
    assert_eq!(texts[4], "🤖 Assistant");
    assert_eq!(texts[5], "");
    assert_eq!(texts[6], "  Final answer");
}

#[test]
fn test_round_footer_button_hit_range_matches_button_text() {
    let mut app = create_embedded_app();
    app.chat_blocks = vec![
        ChatBlock::new(ChatRole::User, "First".to_string()),
        ChatBlock::new(ChatRole::Assistant, "Second".to_string()),
    ];
    app.chat_blocks[0].round_timing_ms = Some(1250);
    app.rebuild_chat_render_state(Rect::new(0, 0, 40, 12));

    let (_, button) = app
        .rendered_round_footer_buttons
        .iter()
        .next()
        .map(|(line_index, button)| (*line_index, *button))
        .expect("footer button should exist");

    assert_eq!(
        button.end_col.saturating_sub(button.start_col),
        COPY_ROUND_BUTTON_TEXT.len()
    );
}

#[test]
fn test_rebuild_chat_render_state_skips_footer_for_non_user_blocks() {
    let mut app = create_embedded_app();
    app.chat_blocks = vec![ChatBlock::new(
        ChatRole::System,
        "Talos TUI (v0.1.0)".to_string(),
    )];
    app.rebuild_chat_render_state(Rect::new(0, 0, 40, 12));

    assert!(app.rendered_round_footer_buttons.is_empty());
}

#[tokio::test]
async fn test_multiple_questions_keep_reply_order() {
    let mut app = create_remote_app();
    app.initialize_remote().await;

    assert!(app.send_user_message("q1".to_string()).await.unwrap());
    assert!(app.send_user_message("q2".to_string()).await.unwrap());
    assert!(app.send_user_message("q3".to_string()).await.unwrap());
    assert!(app.send_user_message("q4".to_string()).await.unwrap());

    for _ in 0..240 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !app.in_flight && app.message_queue.is_empty() {
            break;
        }
    }

    let joined = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .collect::<Vec<_>>()
        .join("\n");

    let p1 = joined.find("reply(q1)").unwrap();
    let p2 = joined.find("reply(q2)").unwrap();
    let p3 = joined.find("reply(q3)").unwrap();
    let p4 = joined.find("reply(q4)").unwrap();
    assert!(p1 < p2 && p2 < p3 && p3 < p4);
}

#[tokio::test]
async fn test_send_is_non_blocking_when_stream_connect_is_slow() {
    let mut app = create_remote_app_with_connect_delay(500);
    app.initialize_remote().await;

    let started = Instant::now();
    assert!(app.send_user_message("slow-1".to_string()).await.unwrap());
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_millis(150));
    assert!(app.in_flight);
    assert!(app.pending_stream_connect.is_some());

    assert!(app.send_user_message("slow-2".to_string()).await.unwrap());
    assert_eq!(app.message_queue.len(), 1);
}

#[tokio::test]
async fn test_send_is_non_blocking_when_session_create_is_slow() {
    let mut app = create_remote_app_with_session_delay(500);

    let started = Instant::now();
    assert!(app
        .send_user_message("slow-create".to_string())
        .await
        .unwrap());
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_millis(150));
    assert_eq!(app.message_queue.len(), 1);
    assert!(app.pending_session_connect.is_some());
    assert!(app.chat_blocks.iter().all(|b| b.content != "slow-create"));
}

#[tokio::test]
async fn test_failed_queued_message_is_dropped_after_one_retry() {
    let mut app = create_remote_app_with_stream_failures(2);
    app.initialize_remote().await;

    assert!(app.send_user_message("drop-me".to_string()).await.unwrap());
    assert!(app.send_user_message("next-one".to_string()).await.unwrap());

    for _ in 0..120 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !app.in_flight && app.message_queue.is_empty() {
            break;
        }
    }

    assert!(app.message_queue.is_empty());
    let user_messages = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::User)
        .map(|b| b.content.clone())
        .collect::<Vec<_>>();
    assert_eq!(user_messages, vec!["next-one".to_string()]);
    let assistant_text = app
        .chat_blocks
        .iter()
        .filter(|b| b.role == ChatRole::Assistant)
        .map(|b| b.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(assistant_text.contains("reply(next-one)"));
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("queued dispatch dropped")));
}

#[tokio::test]
async fn test_quit_enters_closing_state_before_remote_close_finishes() {
    let mut app = create_remote_app_with_close_delay(500);
    app.initialize_remote().await;
    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();

    assert!(!should_quit);
    assert!(app.close_in_progress);
    assert!(app.pending_close_update.is_some());
    assert_eq!(app.connection_status, "closing");
    assert!(app.input_status_line(120).to_string().contains("closing"));
}

#[tokio::test]
async fn test_quit_exits_after_remote_close_completes() {
    let mut app = create_remote_app();
    app.initialize_remote().await;
    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();
    assert!(!should_quit);

    for _ in 0..20 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if app.should_exit {
            break;
        }
    }

    assert!(app.should_exit);
    assert!(!app.close_in_progress);
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("session close completed")));
}

#[tokio::test]
async fn test_quit_timeout_requires_second_quit_to_force_exit() {
    let mut app = create_remote_app_with_close_delay(5_000);
    app.initialize_remote().await;
    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let _ = app.submit_input().await.unwrap();

    app.close_started_at = Some(Instant::now() - SESSION_CLOSE_TIMEOUT - Duration::from_millis(1));
    app.on_tick().await;

    assert!(app.close_in_progress);
    assert!(app.close_timeout_reported);
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("Press /quit again"))
        .unwrap_or(false));

    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let should_quit = app.submit_input().await.unwrap();
    assert!(!should_quit);
    assert!(app.should_exit);
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("session force exit")));
}
