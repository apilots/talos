use super::*;

#[test]
fn test_system_block_lines_keep_multiline_structure() {
    let app = create_embedded_app();
    let block = ChatBlock::new(ChatRole::System, "System ready\nLoading tools".to_string());

    let lines = app.system_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(
        texts,
        vec!["⚙  System", "", "System ready", "Loading tools"]
    );
}

#[test]
fn test_system_block_lines_render_welcome_card_without_prefix() {
    let app = create_embedded_app();
    let block = ChatBlock::new(ChatRole::System, "╭────╮\n│ hi │\n╰────╯".to_string());

    let lines = app.system_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts, vec!["╭────╮", "│ hi │", "╰────╯"]);
}

// ── Phase 4: markdown rendering ──────────────────────────────────────
