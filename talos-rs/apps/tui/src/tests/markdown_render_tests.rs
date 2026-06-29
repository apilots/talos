use super::*;

#[test]
fn test_markdown_bold_renders_bold_span() {
    let lines = render_markdown_lines("Hello **world** text");
    assert_eq!(lines.len(), 1);
    let has_bold = lines[0]
        .spans
        .iter()
        .any(|s| s.content.contains("world") && s.style.add_modifier.contains(Modifier::BOLD));
    assert!(has_bold, "expected 'world' to be bold");
}

#[test]
fn test_markdown_italic_renders_italic_span() {
    let lines = render_markdown_lines("Hello *italic* text");
    assert_eq!(lines.len(), 1);
    let has_italic = lines[0]
        .spans
        .iter()
        .any(|s| s.content.contains("italic") && s.style.add_modifier.contains(Modifier::ITALIC));
    assert!(has_italic, "expected 'italic' to be italic");
}

#[test]
fn test_markdown_inline_code_uses_green_fg() {
    let lines = render_markdown_lines("Run `cargo build` now");
    assert_eq!(lines.len(), 1);
    let has_code = lines[0]
        .spans
        .iter()
        .any(|s| s.content.contains("cargo build") && s.style.fg == Some(Color::LightGreen));
    assert!(has_code, "expected inline code to have LightGreen fg");
}

#[test]
fn test_markdown_header_h1_has_prefix() {
    let lines = render_markdown_lines("# Title");
    assert_eq!(lines.len(), 1);
    let has_prefix = lines[0]
        .spans
        .first()
        .map(|s| s.content.contains('▌'))
        .unwrap_or(false);
    assert!(has_prefix, "H1 should have ▌ prefix span");
}

#[test]
fn test_markdown_list_item_has_bullet() {
    let lines = render_markdown_lines("- item one");
    assert_eq!(lines.len(), 1);
    let has_bullet = lines[0].spans.iter().any(|s| s.content.contains('•'));
    assert!(has_bullet, "list item should have • bullet");
}
