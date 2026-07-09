use super::*;

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

    app.rebuild_chat_render_state(Rect::new(0, 0, 100, 12));
    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts.last().map(String::as_str), Some("✻ Brewing..."));
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

    assert_eq!(height, 6);
}

#[test]
fn test_live_feedback_uses_compact_inline_viewport_without_using_full_screen() {
    let mut app = create_embedded_app();
    app.render_runtime.start_live_text(
        "live-answer",
        crate::render_runtime::render::live::LiveTextKind::FinalAnswer,
        "streaming response",
    );
    app.in_flight = true;

    let height = app.inline_viewport_height(100, 30);

    assert_eq!(height, 7);
    assert!(height < 30);
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

    assert_eq!(height, 6);
}
