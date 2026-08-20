use super::*;
use ratatui::{backend::TestBackend, style::Color, Terminal};

fn interactive_transcript_text(app: &TuiApp) -> String {
    app.render_runtime
        .interactive_projection_lines()
        .into_iter()
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_transcript_answer(app: &mut TuiApp, user: &str, answer: &str) {
    app.render_runtime.append_user_input(user);
    app.render_runtime.append_history([
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                answer,
            )],
        ),
    ]);
}

fn deliver_session_usage_refresh(
    app: &mut TuiApp,
    session_id: &str,
    session: talos_sdk::SessionInfo,
) {
    if let Some(task) = app.pending_session_usage_refresh_task.take() {
        task.abort();
    }
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.try_send(crate::types::SessionUsageRefresh {
        session_id: session_id.to_string(),
        result: Ok(session),
    })
    .expect("ready session usage refresh should fit in channel");
    app.pending_session_usage_refresh = Some(rx);
}

#[test]
fn test_input_status_line_uses_dynamic_token_left_percent() {
    let mut app = create_embedded_app();
    set_test_model_info(&mut app, "kimi-k2.5", 32_768);
    app.available_model_info[0].model.reasoning = true;
    app.current_model = "kimi-k2.5".to_string();
    app.reasoning_effort_override = Some(talos_sdk::ReasoningEffort::High);
    app.session_usage = TokenUsage {
        input_tokens: 964,
        output_tokens: 16,
        total_tokens: 980,
        ..TokenUsage::default()
    };
    app.session_context_usage = talos_sdk::ContextUsage::new(980, 32_768, 30_000, 1);

    let text = line_plain_text(&app.input_status_line(120));

    assert!(text.contains("kimi-k2.5 high"));
    assert!(text.contains("97% left"));
}

#[tokio::test]
async fn test_turn_completed_updates_session_usage_without_inferring_context_usage() {
    let client = std::sync::Arc::new(MockGatewayClient::new());
    let gateway_client: std::sync::Arc<dyn GatewayClient> = client.clone();
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
    set_test_model_info(&mut app, "kimi-k2.5", 32_768);
    app.current_model = "kimi-k2.5".to_string();
    app.in_flight = true;
    app.received_output_delta = false;
    app.session_id = Some("sess-test".to_string());
    app.session_context_usage = talos_sdk::ContextUsage::new(400, 32_768, 30_000, 1);
    {
        let mut state = client.state.lock().unwrap();
        state.session_usage = TokenUsage {
            input_tokens: 1_100,
            output_tokens: 45,
            total_tokens: 1_145,
            cache_creation_input_tokens: Some(7),
            cache_read_input_tokens: Some(13),
        };
        state.session_context_usage = talos_sdk::ContextUsage::new(1_020, 32_768, 30_000, 2);
    }

    app.handle_stream_event(completed_event(
        "msg-usage",
        "sess-test",
        1,
        TokenUsage {
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
    assert_eq!(app.token_left_percent(), None);

    let refreshed_session = client
        .get_session("sess-test")
        .await
        .expect("mock session refresh should succeed");
    deliver_session_usage_refresh(&mut app, "sess-test", refreshed_session.clone());
    app.session_id = Some("sess-other".to_string());
    app.poll_session_usage_refresh();
    assert_eq!(app.session_usage.total_tokens, 980);
    assert_eq!(app.session_context_usage, None);

    app.session_id = Some("sess-test".to_string());
    deliver_session_usage_refresh(&mut app, "sess-test", refreshed_session);
    app.poll_session_usage_refresh();
    assert_eq!(app.session_usage.total_tokens, 1_145);
    assert_eq!(
        app.session_context_usage
            .as_ref()
            .map(|usage| usage.used_tokens),
        Some(1_020)
    );
}

#[tokio::test]
async fn test_live_metrics_do_not_overwrite_session_usage_before_turn_completed() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    set_test_model_info(&mut app, "kimi-k2.5", 1_000);
    app.current_model = "kimi-k2.5".to_string();
    app.session_id = Some("sess-test".to_string());
    app.session_usage = TokenUsage {
        input_tokens: 80,
        output_tokens: 20,
        total_tokens: 100,
        cache_creation_input_tokens: Some(3),
        cache_read_input_tokens: Some(5),
    };

    let turn_usage = TokenUsage {
        input_tokens: 30,
        output_tokens: 10,
        total_tokens: 40,
        cache_creation_input_tokens: Some(2),
        cache_read_input_tokens: Some(4),
    };
    let live_metrics = TurnExecutionMetrics {
        model_rounds: 2,
        tool_calls: 3,
        tool_successes: 2,
        tool_errors: 1,
        stop_reason: StopReason::EndTurn,
        usage: turn_usage.clone(),
        duration_ms: Some(1_250),
    };
    let final_metrics = TurnExecutionMetrics {
        duration_ms: Some(1_500),
        ..live_metrics.clone()
    };

    app.in_flight = true;
    app.active_message = Some("Inspect live metrics".to_string());
    app.handle_stream_event(turn_metrics_event_with_metrics(
        "msg-usage",
        "sess-test",
        1,
        live_metrics.clone(),
    ));

    assert_eq!(app.session_usage.total_tokens, 100);
    assert_eq!(app.turn_metrics, Some(live_metrics));
    assert!(app.turn_metrics_live);
    assert_eq!(app.displayed_usage().total_tokens, 140);
    assert_eq!(app.displayed_usage().cache_creation_input_tokens, Some(5));
    assert_eq!(app.displayed_usage().cache_read_input_tokens, Some(9));
    assert_eq!(app.token_left_percent(), None);
    let live_metrics_line = app
        .turn_metrics_status_line(180)
        .map(|line| line_plain_text(&line))
        .unwrap();
    assert!(
        live_metrics_line.contains("live · 2 rounds · 3 tools · 2 ok · 1 err · 40 tokens · 1.2s")
    );
    assert!(!live_metrics_line.contains("Brewing"));
    let input_status = line_plain_text(&app.input_status_line(180));
    assert!(!input_status.contains('%'));
    assert!(!input_status.contains("Brewing"));

    app.handle_stream_event(completed_event_with_metrics(
        "msg-usage",
        "sess-test",
        2,
        turn_usage,
        final_metrics.clone(),
    ));

    assert_eq!(app.session_usage.total_tokens, 140);
    assert_eq!(app.session_usage.cache_creation_input_tokens, Some(5));
    assert_eq!(app.session_usage.cache_read_input_tokens, Some(9));
    assert_eq!(app.turn_metrics, Some(final_metrics));
    assert!(!app.turn_metrics_live);
    assert_eq!(app.token_left_percent(), None);
    let final_metrics_line = app
        .turn_metrics_status_line(180)
        .map(|line| line_plain_text(&line))
        .unwrap();
    assert!(
        final_metrics_line.contains("last · 2 rounds · 3 tools · 2 ok · 1 err · 40 tokens · 1.5s")
    );
    assert!(!final_metrics_line.contains("Brewing"));
}

#[tokio::test]
async fn test_completed_without_final_metrics_preserves_last_live_metrics() {
    let mut app = create_app_server_app();
    app.initialize_session().await;
    set_test_model_info(&mut app, "kimi-k2.5", 1_000);
    app.current_model = "kimi-k2.5".to_string();
    app.session_id = Some("sess-test".to_string());

    let turn_usage = TokenUsage {
        input_tokens: 30,
        output_tokens: 10,
        total_tokens: 40,
        ..TokenUsage::default()
    };
    let live_metrics = TurnExecutionMetrics {
        model_rounds: 2,
        tool_calls: 3,
        tool_successes: 2,
        tool_errors: 1,
        stop_reason: StopReason::EndTurn,
        usage: turn_usage.clone(),
        duration_ms: Some(1_250),
    };

    app.handle_stream_event(turn_metrics_event_with_metrics(
        "msg-usage",
        "sess-test",
        1,
        live_metrics.clone(),
    ));
    app.handle_stream_event(completed_event("msg-usage", "sess-test", 2, turn_usage));

    assert_eq!(app.turn_metrics, Some(live_metrics));
    assert!(!app.turn_metrics_live);
    let final_metrics_line = app
        .turn_metrics_status_line(180)
        .map(|line| line_plain_text(&line))
        .unwrap();
    assert!(final_metrics_line.contains("last · 2 rounds · 3 tools · 2 ok · 1 err"));
}

#[test]
fn test_metrics_line_renders_above_input_without_border() {
    let mut app = create_embedded_app();
    set_test_model_info(&mut app, "kimi-k2.5", 128_000);
    app.current_model = "kimi-k2.5".to_string();
    app.turn_metrics = Some(TurnExecutionMetrics {
        model_rounds: 2,
        tool_calls: 3,
        tool_successes: 2,
        tool_errors: 1,
        stop_reason: StopReason::EndTurn,
        usage: TokenUsage {
            total_tokens: 4_200,
            ..TokenUsage::default()
        },
        duration_ms: Some(12_300),
    });
    app.turn_metrics_live = true;
    app.in_flight = true;
    app.active_message = Some("Ask Talos".to_string());
    app.input_buffer = "Ask Talos".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let backend = TestBackend::new(110, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render_ui(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let lines = buffer_lines(buffer);
    let metrics_index = lines
        .iter()
        .position(|line| {
            line.contains("live · 2 rounds · 3 tools · 2 ok · 1 err · 4.2k tokens · 12.3s")
        })
        .unwrap();
    let brewing_index = lines
        .iter()
        .position(|line| line.contains("✻ Brewing..."))
        .unwrap();
    let input_index = lines
        .iter()
        .position(|line| line.contains("❯ Ask Talos"))
        .unwrap();
    let status_index = lines
        .iter()
        .position(|line| line.contains("kimi-k2.5") && line.contains("left"))
        .unwrap();

    assert!(brewing_index < metrics_index);
    assert!(!lines[metrics_index].contains("Brewing"));
    assert!(metrics_index < input_index);
    assert!(input_index < status_index);
    let inner_metrics = lines[metrics_index].trim_matches('│');
    assert!(inner_metrics.trim_end().ends_with("12.3s"));
    assert_eq!(buffer[(0, metrics_index as u16)].bg, Color::Reset);
    assert_eq!(buffer[(0, status_index as u16)].bg, Color::Reset);
}

#[test]
fn test_status_block_renders_without_system_header_or_round_footer() {
    let mut app = create_embedded_app();
    app.render_runtime.append_command_input("/status");
    app.render_runtime
        .append_system_notice("┌────┐\n│ Model:            qwen-plus │\n└────┘");

    app.rebuild_chat_render_state(Rect::new(0, 0, 100, 20));

    let rendered = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(rendered.matches("/status").count(), 1);
    assert!(rendered.contains("Model:"));
    assert!(!rendered.contains("System"));
}

fn buffer_lines(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn test_synced_long_transcript_stays_out_of_inline_viewport() {
    let mut app = create_embedded_app();
    for index in 0..200 {
        app.render_runtime
            .append_system_notice(format!("notice {index:03}"));
    }
    let synced_block_ids = app
        .render_runtime
        .pending_terminal_projection()
        .into_iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    app.render_runtime
        .mark_terminal_blocks_synced(synced_block_ids);

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render_ui(frame)).unwrap();
    let rendered = buffer_lines(terminal.backend().buffer()).join("\n");

    assert!(!rendered.contains("notice 000"));
    assert_eq!(app.inline_viewport_height(100, 30), 7);
}

#[test]
fn test_empty_inline_viewport_does_not_paint_full_background() {
    let mut app = create_embedded_app();
    app.current_model = "qwen-test".to_string();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| app.render_ui(frame)).unwrap();

    let buffer = terminal.backend().buffer();
    let lines = buffer_lines(buffer);
    let rendered = lines.join("\n");
    assert!(rendered.contains("❯"));
    assert!(!rendered.contains("Talos TUI"));
    assert_eq!(buffer[(79, 0)].bg, Color::Reset);
}

#[test]
fn test_synced_history_without_live_content_keeps_input_below_idle_gap() {
    let mut app = create_embedded_app();
    app.sync_session_header();
    let synced_block_ids = app
        .render_runtime
        .pending_terminal_projection()
        .into_iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    app.render_runtime
        .mark_terminal_blocks_synced(synced_block_ids);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render_ui(frame)).unwrap();

    let lines = buffer_lines(terminal.backend().buffer());
    let input_index = lines
        .iter()
        .position(|line| line.contains("❯"))
        .expect("input prompt should render");
    let input_area = app.input_area.expect("composer should record input area");
    let centered_prompt_row = input_area.y + (input_area.height.saturating_sub(1) / 2);

    assert_eq!(input_index, centered_prompt_row as usize);
}

#[test]
fn test_session_header_remains_top_after_first_round() {
    let mut app = create_embedded_app();
    app.current_model = "qwen-test".to_string();
    app.current_project_root = Some(PathBuf::from("/tmp/talos-workspace"));
    app.sync_session_header();
    append_transcript_answer(&mut app, "你好", "你好！有什么需要帮忙的？");

    app.rebuild_chat_render_state(Rect::new(0, 0, 100, 16));

    let rendered = app
        .visible_chat_window()
        .lines
        .iter()
        .map(line_plain_text)
        .collect::<Vec<_>>();
    assert!(rendered[0].contains("╭──────────────────────────────────────────────────╮"));
    assert!(rendered.iter().any(|line| line.contains("Talos TUI")));
    assert!(rendered.iter().any(|line| line.contains("● 你好")));
    assert!(rendered
        .iter()
        .any(|line| line.contains("你好！有什么需要帮忙的？")));
}

#[test]
fn test_live_round_is_separated_from_synced_history() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "history user", "history answer");
    let synced_block_ids = app
        .render_runtime
        .pending_terminal_projection()
        .into_iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    app.render_runtime
        .mark_terminal_blocks_synced(synced_block_ids);
    start_live_text(
        &mut app.render_runtime,
        "item-live",
        crate::render_runtime::render::live::LiveTextKind::FinalAnswer,
        "live answer",
    );
    app.in_flight = true;

    let viewport_height = app.inline_viewport_height(60, 30);
    let backend = TestBackend::new(60, viewport_height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render_ui(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let texts = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(texts[0].trim().is_empty());
    assert!(texts[1].contains("● live answer"));
}

#[test]
fn test_session_header_enters_terminal_projection() {
    let mut app = create_embedded_app();
    app.current_model = "qwen-test".to_string();
    app.current_project_root = Some(PathBuf::from("/tmp/talos-workspace"));

    app.sync_session_header();

    let pending = app.render_runtime.pending_terminal_projection();
    let text = pending
        .iter()
        .flat_map(|block| block.lines.iter())
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Talos TUI"));
    assert!(text.contains("qwen-test"));
    assert!(text.contains(
        " Tip: Type a message, Ctrl+V an image, or use /help /model /permissions /sessions"
    ));
}

#[test]
fn test_exit_snapshot_renders_transcript_without_interaction_controls() {
    let mut app = create_embedded_app();
    app.current_model = "qwen-test".to_string();
    app.turn_metrics = Some(TurnExecutionMetrics {
        model_rounds: 1,
        tool_calls: 1,
        tool_successes: 1,
        tool_errors: 0,
        stop_reason: StopReason::EndTurn,
        duration_ms: Some(1000),
        usage: TokenUsage::default(),
    });
    app.input_buffer = "draft should not render".to_string();
    app.should_exit = true;
    append_transcript_answer(&mut app, "exit user", "exit answer");
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| app.render_ui(frame)).unwrap();

    let rendered = buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(rendered.contains("exit user"));
    assert!(rendered.contains("exit answer"));
    assert!(!rendered.contains("draft should not render"));
    assert!(!rendered.contains("qwen-test"));
    assert!(!rendered.contains("last ·"));
}

#[test]
fn test_rebuild_chat_render_state_renders_round_footer() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "First", "Second");
    app.rebuild_chat_render_state(Rect::new(0, 0, 40, 12));

    let footer_text = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .find(|line| line.contains("✻ Brewed"))
        .expect("footer should exist");
    assert!(footer_text.contains("✻ Brewed for a moment"));
}

#[test]
fn test_rebuild_chat_render_state_hides_round_footer_while_turn_is_in_flight() {
    let mut app = create_embedded_app();
    app.in_flight = true;
    app.render_runtime.append_user_input("running user");
    app.render_runtime.append_history([
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::Thinking,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "still thinking",
            )],
        ),
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::Tool,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "● Bash(running)",
            )],
        ),
    ]);

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 12));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("running user"));
    assert!(joined.contains("Brewing"));
    assert!(!joined.contains("Brewed"));
}

#[test]
fn test_rebuild_chat_render_state_uses_completed_turn_duration_in_footer() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "Timed", "Done");
    app.turn_metrics = Some(TurnExecutionMetrics {
        model_rounds: 1,
        tool_calls: 0,
        tool_successes: 0,
        tool_errors: 0,
        stop_reason: StopReason::EndTurn,
        usage: TokenUsage::default(),
        duration_ms: Some(12_300),
    });

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 12));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("✻ Brewed for 12s"));
    assert!(!joined.contains("✻ Brewed for a moment"));
}

#[test]
fn test_completed_footer_is_separated_from_next_round() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "First", "Second");
    append_transcript_answer(&mut app, "Third", "Fourth");

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 20));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    let footer_index = texts
        .iter()
        .position(|line| line.contains("✻ Brewed"))
        .expect("completed footer should render");
    let next_round_index = texts
        .iter()
        .position(|line| line == "● Third")
        .expect("next round should render");

    assert_eq!(texts[footer_index + 1], "");
    assert_eq!(next_round_index, footer_index + 2);
}

#[test]
fn test_completed_footer_does_not_leave_trailing_blank_line() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "First", "Second");

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 12));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.last().is_some_and(|line| line.contains("✻ Brewed")));
}

#[test]
fn test_rebuild_chat_render_state_inserts_spacing_between_blocks() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "Execution summary", "Final answer");

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts[0], "● Execution summary");
    assert_eq!(texts[1], "");
    assert_eq!(texts[2], "● Final answer");
}

#[test]
fn test_command_input_renders_without_message_bullet() {
    let mut app = create_embedded_app();
    app.render_runtime.append_command_input("/quit");

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.iter().any(|line| line == "/quit"));
    assert!(!texts.iter().any(|line| line == "● /quit"));
}

#[test]
fn test_command_input_separates_model_rounds() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "First", "Second");
    app.render_runtime.append_command_input("/status");
    append_transcript_answer(&mut app, "Third", "Fourth");

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 24));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    let first_footer_index = texts
        .iter()
        .position(|line| line.contains("✻ Brewed"))
        .expect("first round footer should render before command");
    let command_index = texts
        .iter()
        .position(|line| line == "/status")
        .expect("command should render");
    let next_round_index = texts
        .iter()
        .position(|line| line == "● Third")
        .expect("next round should render after command");

    assert!(first_footer_index < command_index);
    assert_eq!(texts[first_footer_index + 1], "");
    assert_eq!(command_index, first_footer_index + 2);
    assert!(command_index < next_round_index);
}

#[test]
fn test_rebuild_chat_render_state_cache_invalidates_on_content_change() {
    let mut app = create_embedded_app();
    app.render_runtime.append_system_notice("Before");
    let rect = Rect::new(0, 0, 60, 12);

    app.rebuild_chat_render_state(rect);
    let first_key = app.rendered_chat_cache_key;
    app.render_runtime.append_system_notice("After");
    app.rebuild_chat_render_state(rect);

    assert_ne!(app.rendered_chat_cache_key, first_key);
    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("After"));
}

#[test]
fn test_visible_chat_window_is_capped_by_viewport_and_overscan() {
    let mut app = create_embedded_app();
    for index in 0..200 {
        app.render_runtime
            .append_system_notice(format!("notice {index:03}"));
    }
    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 10));
    assert!(app.rendered_chat_lines.len() > 300);

    let window = app.visible_chat_window_with_overscan(120, 10, 2);

    assert_eq!(window.lines.len(), 14);
    assert_eq!(window.local_scroll, 2);
    assert_eq!(
        line_plain_text(&window.lines[0]),
        line_plain_text(&app.rendered_chat_lines[118].display)
    );
    assert_eq!(
        line_plain_text(&window.lines[2]),
        line_plain_text(&app.rendered_chat_lines[120].display)
    );
    assert_eq!(
        line_plain_text(&window.lines[13]),
        line_plain_text(&app.rendered_chat_lines[131].display)
    );
}

#[test]
fn test_rebuild_chat_render_state_uses_transcript_projection() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "transcript user", "transcript answer");

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("transcript user"));
    assert!(joined.contains("transcript answer"));
    assert!(joined.contains("✻ Brewed for a moment"));
}

#[test]
fn test_final_answer_preserves_newlines_with_message_bullet() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("summarize");
    app.render_runtime.append_history([
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "# Title\n\n- first\n- second",
            )],
        ),
    ]);

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 20));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|line| line == "● # Title"));
    assert!(texts.iter().any(|line| line == "  - first"));
    assert!(texts.iter().any(|line| line == "  - second"));
}

#[test]
fn test_completed_commentary_preserves_authoritative_kind() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("summarize");
    app.render_runtime.append_history([
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::Commentary,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "I will inspect the workspace.",
            )],
        ),
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::Commentary,
            crate::render_runtime::common::style::StyledLine::plain_lines(
                "# Summary\n\n- first\n- second",
            ),
        ),
    ]);

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 20));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    assert!(app.render_runtime.store().blocks()[1..]
        .iter()
        .all(|block| block.kind
            == crate::render_runtime::transcript::block::TranscriptBlockKind::Commentary));
    assert!(texts
        .iter()
        .any(|line| line == "● I will inspect the workspace."));
    assert!(texts.iter().any(|line| line == "● # Summary"));
    assert!(texts.iter().any(|line| line == "  - first"));
    assert!(texts.iter().any(|line| line == "  - second"));
}

#[test]
fn test_completed_round_preserves_thinking_before_final_answer() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("analyze");
    app.render_runtime.append_history([
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::Thinking,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "reasoning preview",
            )],
        ),
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "final response",
            )],
        ),
    ]);

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 20));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    let thinking_index = texts
        .iter()
        .position(|line| line.contains("Thinking: reasoning preview"))
        .expect("thinking should render");
    let answer_index = texts
        .iter()
        .position(|line| line == "● final response")
        .expect("final answer should render");

    assert!(thinking_index < answer_index);
}

#[test]
fn test_terminal_synced_transcript_is_not_duplicated_in_tui_viewport() {
    let mut app = create_embedded_app();
    append_transcript_answer(&mut app, "synced user", "synced answer");
    let synced_block_ids = app
        .render_runtime
        .pending_terminal_projection()
        .into_iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    app.render_runtime
        .mark_terminal_blocks_synced(synced_block_ids);
    append_transcript_answer(&mut app, "visible user", "visible answer");

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let non_empty_lines = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        non_empty_lines,
        vec![
            "● visible user".to_string(),
            "● visible answer".to_string(),
            "✻ Brewed for a moment".to_string(),
        ]
    );
}

#[test]
fn test_rebuild_chat_render_state_uses_status_projection() {
    let mut app = create_embedded_app();
    app.render_runtime.append_system_notice("transcript status");

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("transcript status"));
}

#[test]
fn test_rebuild_chat_render_state_uses_live_tail_when_available() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("transcript user");
    start_live_text(
        &mut app.render_runtime,
        "item-live",
        crate::render_runtime::render::live::LiveTextKind::FinalAnswer,
        "live answer",
    );
    app.in_flight = true;

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("transcript user"));
    assert!(joined.contains("live answer"));
    assert!(joined.contains("Brewing"));
    assert!(!joined.contains("Brewed"));
}

#[test]
fn test_live_activity_line_is_separated_from_live_tail() {
    let mut app = create_embedded_app();
    start_live_text(
        &mut app.render_runtime,
        "item-live",
        crate::render_runtime::render::live::LiveTextKind::FinalAnswer,
        "live answer",
    );
    app.in_flight = true;

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    let live_index = texts
        .iter()
        .position(|line| line == "● live answer")
        .expect("live answer should render");
    let brewing_index = texts
        .iter()
        .position(|line| line.contains("Brewing"))
        .expect("brewing line should render");

    assert_eq!(texts[live_index + 1], "");
    assert_eq!(brewing_index, live_index + 2);
}

#[test]
fn test_live_model_feedback_preserves_kind_specific_rendering() {
    let mut app = create_embedded_app();
    start_live_text(
        &mut app.render_runtime,
        "thinking-live",
        crate::render_runtime::render::live::LiveTextKind::Thinking,
        "inspect files",
    );
    start_live_text(
        &mut app.render_runtime,
        "commentary-live",
        crate::render_runtime::render::live::LiveTextKind::Commentary,
        "Reading project",
    );
    app.in_flight = true;

    app.rebuild_chat_render_state(Rect::new(0, 0, 80, 12));

    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    let joined = texts.join("\n");
    assert!(joined.contains("∴ Thinking: inspect files (ctrl+o to expand)"));
    assert!(joined.contains("● Reading project"));
    assert!(joined.contains("Brewing"));
    assert!(!joined.contains("Brewed"));
    let thinking_index = texts
        .iter()
        .position(|line| line.contains("Thinking: inspect files"))
        .unwrap();
    let commentary_index = texts
        .iter()
        .position(|line| line == "● Reading project")
        .unwrap();
    assert_eq!(texts[thinking_index + 1], "");
    assert_eq!(commentary_index, thinking_index + 2);
}

#[test]
fn test_rebuild_chat_render_state_uses_active_tool_snapshot_when_available() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("transcript user");
    start_live_tool(
        &mut app.render_runtime,
        "tool-1",
        "call-1",
        "Read",
        r#"{"file_path":"/tmp/demo.rs"}"#,
    );
    append_live_tool_progress(&mut app.render_runtime, "tool-1", "Read 12 lines");
    app.in_flight = true;

    app.rebuild_chat_render_state(Rect::new(0, 0, 60, 12));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("transcript user"));
    assert!(joined.contains("✻ Read(/tmp/demo.rs)"));
    assert!(joined.contains("  ⎿ Read 12 lines"));
    assert!(joined.contains("Brewing"));
    assert!(!joined.contains("Brewed"));
}

#[test]
fn test_completed_tool_rendering_is_collapsed_by_default() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("find prompts");
    app.render_runtime.append_history([
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::Tool,
            vec![
                crate::render_runtime::common::style::StyledLine::plain(
                    "● Glob(**/*.rs) completed",
                ),
                crate::render_runtime::common::style::StyledLine::plain("  ⎿ Found 28 matches"),
                crate::render_runtime::common::style::StyledLine::plain(
                    "    common/prompt/src/assembler.rs:1 common/prompt/src/assembler.rs",
                ),
                crate::render_runtime::common::style::StyledLine::plain(
                    "    common/prompt/src/bootstrap.rs:1 common/prompt/src/bootstrap.rs",
                ),
            ],
        ),
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "done",
            )],
        ),
    ]);

    app.rebuild_chat_render_state(Rect::new(0, 0, 100, 16));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("● Glob(**/*.rs) completed"));
    assert!(joined.contains("⎿ Found 28 matches (ctrl+o to expand)"));
    assert!(!joined.contains("common/prompt/src/assembler.rs"));
    assert!(!joined.contains("common/prompt/src/bootstrap.rs"));
}

#[test]
fn test_completed_diff_tool_rendering_is_expanded_by_default() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("edit file");
    app.render_runtime.append_history([
        crate::render_runtime::render::tool_projection::tool_completed_block(
            "Edit",
            "call-edit",
            r#"{"file_path":"/tmp/demo.rs"}"#,
            false,
            &[talos_sdk::TalosContentItem::Diff {
                files: vec![talos_types::talos::TalosToolFileDiff {
                    path: "/tmp/demo.rs".to_string(),
                    old_path: None,
                    hunks: vec![talos_types::talos::TalosToolDiffHunk {
                        old_start: Some(1),
                        old_lines: Some(2),
                        new_start: Some(1),
                        new_lines: Some(2),
                        lines: vec![
                            talos_types::talos::TalosToolDiffLine::Context {
                                text: "fn main() {".to_string(),
                            },
                            talos_types::talos::TalosToolDiffLine::Removed {
                                text: "    old();".to_string(),
                            },
                            talos_types::talos::TalosToolDiffLine::Added {
                                text: "    new();".to_string(),
                            },
                        ],
                    }],
                    truncated: false,
                }],
            }],
        ),
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "done",
            )],
        ),
    ]);

    app.rebuild_chat_render_state(Rect::new(0, 0, 100, 20));

    let joined = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("● Edited 1 files (+1 -1)"));
    assert!(joined.contains("/tmp/demo.rs (+1 -1)"));
    assert!(joined.contains("@@ -1,2 +1,2 @@"));
    assert!(joined.contains("-    old();"));
    assert!(joined.contains("+    new();"));
    assert!(!joined.contains("ctrl+o to expand"));
}

#[test]
fn test_rebuild_chat_render_state_skips_footer_for_non_user_blocks() {
    let mut app = create_embedded_app();
    app.render_runtime
        .append_system_notice("Talos TUI (v0.1.5)");
    app.rebuild_chat_render_state(Rect::new(0, 0, 40, 12));
}

#[tokio::test]
async fn test_multiple_questions_keep_server_queue_order() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("q1".to_string()).await.unwrap());
    assert!(app.send_user_message("q2".to_string()).await.unwrap());
    assert!(app.send_user_message("q3".to_string()).await.unwrap());
    assert!(app.send_user_message("q4".to_string()).await.unwrap());

    let queued_text = app
        .message_queue
        .messages
        .iter()
        .map(crate::message_queue::queued_message_text)
        .collect::<Vec<_>>();
    assert_eq!(queued_text, ["q2", "q3", "q4"]);
}

#[tokio::test]
async fn test_send_is_non_blocking_when_stream_connect_is_slow() {
    let mut app = create_app_server_app_with_connect_delay(500);
    app.initialize_session().await;

    let started = Instant::now();
    assert!(app.send_user_message("slow-1".to_string()).await.unwrap());
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_millis(150));
    assert!(app.in_flight);
    assert!(app.pending_stream_connect.is_some());

    assert!(app.send_user_message("slow-2".to_string()).await.unwrap());
    assert_eq!(app.message_queue.messages.len(), 1);
}

#[tokio::test]
async fn test_send_is_non_blocking_for_first_draft_turn() {
    let mut app = create_app_server_app();

    let started = Instant::now();
    assert!(app
        .send_user_message("slow-create".to_string())
        .await
        .unwrap());
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_millis(150));
    assert_eq!(app.message_queue.messages.len(), 0);
    assert!(app.pending_stream_connect.is_some());
    assert!(interactive_transcript_text(&app).contains("slow-create"));
}

#[tokio::test]
async fn test_failed_active_turn_does_not_drop_queued_message() {
    let mut app = create_app_server_app_with_stream_failures(2);
    app.initialize_session().await;

    assert!(app.send_user_message("drop-me".to_string()).await.unwrap());
    assert!(app.send_user_message("next-one".to_string()).await.unwrap());

    for _ in 0..120 {
        app.on_tick().await;
        tokio::task::yield_now().await;
        if !app.in_flight {
            break;
        }
    }

    assert_eq!(app.message_queue.messages.len(), 1);
    assert_eq!(
        crate::message_queue::queued_message_text(&app.message_queue.messages[0]),
        "next-one"
    );
}

#[tokio::test]
async fn test_quit_enters_closing_state_before_session_close_finishes() {
    let mut app = create_app_server_app_with_close_delay(500);
    app.initialize_session().await;
    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();

    let should_quit = app.submit_input().await.unwrap();

    assert!(!should_quit);
    assert!(app.close_in_progress);
    assert!(app.pending_close_update.is_some());
    assert_eq!(app.connection_status, "closing");
    assert!(app.input_status_line(120).to_string().contains("closing"));
    assert!(app.render_runtime.store().blocks().iter().any(|block| {
        block.kind == crate::render_runtime::transcript::block::TranscriptBlockKind::CommandInput
            && block
                .lines
                .iter()
                .any(|line| line.content.plain_text() == "/quit")
    }));
    assert!(!app.render_runtime.store().blocks().iter().any(|block| {
        block.kind == crate::render_runtime::transcript::block::TranscriptBlockKind::UserInput
            && block
                .lines
                .iter()
                .any(|line| line.content.plain_text() == "/quit")
    }));
}

#[tokio::test]
async fn test_quit_exits_after_session_close_completes() {
    let client = std::sync::Arc::new(MockGatewayClient::new());
    let state = client.state.clone();
    let gateway_client: std::sync::Arc<dyn GatewayClient> = client;
    let mut app = TuiApp::new_with_gateway_client(gateway_client);
    app.initialize_session().await;
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
    assert!(app.client_shutdown_completed);
    assert_eq!(state.lock().unwrap().shutdown_calls, 1);
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("session close completed")));
}

#[tokio::test]
async fn test_slow_quit_reports_progress_before_force_exit_deadline() {
    let mut app = create_app_server_app_with_close_delay(70_000);
    app.initialize_session().await;
    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let _ = app.submit_input().await.unwrap();

    app.close_started_at =
        Some(Instant::now() - SESSION_CLOSE_SLOW_NOTICE - Duration::from_millis(1));
    app.on_tick().await;

    assert!(app.close_in_progress);
    assert!(app.close_slow_notice_reported);
    assert!(!app.can_force_close());
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("Session close is still in progress"))
        .unwrap_or(false));
    assert!(!app
        .right_panel_events
        .iter()
        .any(|line| line.contains("timed out")));

    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let should_quit = app.submit_input().await.unwrap();
    assert!(!should_quit);
    assert!(!app.should_exit);

    app.close_started_at =
        Some(Instant::now() - SESSION_CLOSE_FORCE_TIMEOUT - Duration::from_millis(1));
    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let should_quit = app.submit_input().await.unwrap();
    assert!(!should_quit);
    assert!(app.should_exit);
    assert!(app.force_exit_without_shutdown);
    assert!(app
        .right_panel_events
        .iter()
        .any(|line| line.contains("session force exit")));
}
