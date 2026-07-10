use super::*;
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn test_in_flight_activity_line_stays_visible_after_transcript_activity() {
    let mut app = create_embedded_app();
    app.in_flight = true;
    app.render_runtime
        .append_system_notice("I will inspect the files.");

    app.rebuild_chat_render_state(Rect::new(0, 0, 100, 12));
    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts.last().map(String::as_str), Some("✻ Brewing..."));
}

#[test]
fn test_in_flight_activity_line_stays_visible_with_live_metrics() {
    let mut app = create_embedded_app();
    app.in_flight = true;
    app.turn_metrics_live = true;
    app.turn_metrics = Some(TurnExecutionMetrics::default());
    app.render_runtime
        .append_system_notice("I will inspect the files.");

    let backend = TestBackend::new(100, 12);
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
    let activity_index = texts
        .iter()
        .position(|line| line.contains("Brewing"))
        .expect("live activity should render");
    let metrics_index = texts
        .iter()
        .position(|line| line.contains("live ·"))
        .expect("live metrics should render");

    assert!(texts[activity_index + 1].trim().is_empty());
    assert_eq!(metrics_index, activity_index + 2);
}

#[test]
fn test_chat_content_rect_uses_full_width_without_left_padding() {
    let app = create_embedded_app();
    let chat_rect = app.chat_content_rect(Rect::new(1, 1, 20, 6));

    assert_eq!(chat_rect, Rect::new(1, 1, 20, 6));
}

#[test]
fn test_synced_history_uses_fixed_idle_inline_viewport() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("hello");
    app.render_runtime.append_history([
        crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            crate::render_runtime::transcript::block::TranscriptBlockKind::FinalAnswer,
            vec![crate::render_runtime::common::style::StyledLine::plain(
                "done",
            )],
        ),
    ]);
    let synced_block_ids = app
        .render_runtime
        .pending_terminal_projection()
        .into_iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    app.render_runtime
        .mark_terminal_blocks_synced(synced_block_ids);

    let height = app.inline_viewport_height(100, 30);

    assert_eq!(height, 7);
}

#[test]
fn test_composer_layout_is_stable_when_session_metrics_change() {
    let mut app = create_embedded_app();
    app.render_runtime.append_user_input("hello");
    let synced_block_ids = app
        .render_runtime
        .pending_terminal_projection()
        .into_iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    app.render_runtime
        .mark_terminal_blocks_synced(synced_block_ids);
    let without_metrics = app.inline_viewport_height(100, 30);

    app.turn_metrics = Some(TurnExecutionMetrics::default());
    let with_metrics = app.inline_viewport_height(100, 30);

    assert_eq!(with_metrics, without_metrics);
    assert_eq!(with_metrics, 7);
}

#[test]
fn test_composer_layout_owns_metrics_dynamic_input_and_status_rows() {
    let layout = crate::render::composer::ComposerLayout::new(
        Rect::new(0, 10, 100, 7),
        /*input_height*/ 5,
    );

    assert_eq!(layout.metrics, Rect::new(0, 10, 100, 1));
    assert_eq!(layout.input, Rect::new(0, 11, 100, 5));
    assert_eq!(layout.status, Rect::new(0, 16, 100, 1));
}

#[test]
fn test_composer_input_height_grows_from_three_to_eight_rows() {
    let mut app = create_embedded_app();
    assert_eq!(crate::render::composer::input_height(&app, 40), 3);

    app.input_buffer = "one\ntwo\nthree\nfour\nfive".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    assert_eq!(crate::render::composer::input_height(&app, 40), 5);

    app.input_buffer = (0..12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.cursor_index = app.input_buffer.chars().count();
    assert_eq!(crate::render::composer::input_height(&app, 40), 8);
}

#[test]
fn test_compact_paste_marker_keeps_default_input_height() {
    let mut app = create_embedded_app();
    app.handle_paste("x".repeat(crate::PASTED_BLOCK_CHAR_THRESHOLD + 1));

    assert_eq!(crate::render::composer::input_height(&app, 80), 3);
}

#[test]
fn test_live_feedback_uses_compact_inline_viewport_without_using_full_screen() {
    let mut app = create_embedded_app();
    start_live_text(
        &mut app.render_runtime,
        "live-answer",
        crate::render_runtime::render::live::LiveTextKind::FinalAnswer,
        "streaming response",
    );
    app.in_flight = true;

    let height = app.inline_viewport_height(100, 30);

    assert_eq!(height, 9);
    assert!(height < 30);
}

#[test]
fn test_session_picker_uses_full_terminal_viewport() {
    let mut app = create_embedded_app();
    app.session_picker = Some(crate::session_picker::SessionPickerState::loading());

    let height = app.inline_viewport_height(100, 30);

    assert_eq!(height, 30);
}

#[test]
fn test_pending_execution_transcript_does_not_expand_live_viewport() {
    let mut app = create_embedded_app();
    app.in_flight = true;
    app.render_runtime.append_user_input("analyze");
    for index in 0..12 {
        app.render_runtime
            .append_system_notice(format!("tool summary {index}"));
    }

    let height = app.inline_viewport_height(100, 30);

    assert_eq!(height, 8);
}
