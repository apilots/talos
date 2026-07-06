use super::*;

#[test]
fn test_final_answer_block_preserves_raw_transcript_text() {
    let app = create_embedded_app();
    let block = ChatBlock::new(
        ChatRole::FinalAnswer,
        "## Summary\n\n---\n\n1. First\n2. Second\n\n┌──┐\n│ok│\n└──┘\n\n```rust\nlet x = 1;\n```"
            .to_string(),
    );

    let texts = app
        .final_answer_block_lines(&block, 80)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(
        texts,
        vec![
            "## Summary",
            "",
            "---",
            "",
            "1. First",
            "2. Second",
            "",
            "┌──┐",
            "│ok│",
            "└──┘",
            "",
            "```rust",
            "let x = 1;",
            "```",
        ]
    );
}

#[test]
fn test_user_and_thinking_blocks_use_bullet_transcript_style() {
    let app = create_embedded_app();
    let user = ChatBlock::new(ChatRole::User, "分析当前代码架构".to_string());
    let thinking = ChatBlock::new(ChatRole::Thinking, "先看 TUI 渲染路径".to_string());

    let user_texts = app
        .user_block_lines(&user, 80)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();
    let thinking_texts = app
        .thinking_block_lines(&thinking, 80)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(user_texts, vec!["● 分析当前代码架构"]);
    assert_eq!(
        thinking_texts,
        vec!["∴ Thinking: 先看 TUI 渲染路径 (ctrl+o to expand)"]
    );
}

#[test]
fn test_in_flight_activity_line_stays_visible_after_transcript_activity() {
    let mut app = create_embedded_app();
    app.in_flight = true;
    app.push_block(
        ChatRole::FinalAnswer,
        "I will inspect the files.".to_string(),
    );

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
    app.push_block(
        ChatRole::FinalAnswer,
        "I will inspect the files.".to_string(),
    );

    app.rebuild_chat_render_state(Rect::new(0, 0, 100, 12));
    let texts = app
        .rendered_chat_lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts.last().map(String::as_str), Some("✻ Brewing..."));
}

#[test]
fn test_chat_content_columns_keep_left_padding() {
    let app = create_embedded_app();
    let (chat_rect, scrollbar_rect) = app.chat_content_columns(Rect::new(1, 1, 20, 6));

    assert_eq!(chat_rect, Rect::new(3, 1, 17, 6));
    assert_eq!(scrollbar_rect, Rect::new(20, 1, 1, 6));
}
