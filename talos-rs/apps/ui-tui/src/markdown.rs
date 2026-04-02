//! Markdown rendering for TUI chat blocks.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render a markdown string into a list of styled ratatui `Line`s.
///
/// Supports code fences, H1–H3 headings, blockquotes, unordered and ordered
/// lists, and inline **bold**, *italic*, `code`.
pub(crate) fn render_markdown_lines(markdown: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw in markdown.lines() {
        let trimmed = raw.trim_start();

        // Code block fence
        if trimmed.starts_with("```") {
            if !in_code_block {
                let lang = trimmed.trim_start_matches('`').trim();
                let label = if lang.is_empty() {
                    "─── code ───".to_string()
                } else {
                    format!("─── {lang} ───")
                };
                lines.push(Line::from(Span::styled(
                    label,
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "────────────".to_string(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default()
                    .fg(Color::LightGreen)
                    .bg(Color::Rgb(28, 32, 40)),
            )));
            continue;
        }

        // H3
        if let Some(rest) = trimmed.strip_prefix("### ") {
            let mut spans = vec![Span::styled(
                "▌ ".to_string(),
                Style::default().fg(Color::Rgb(150, 150, 200)),
            )];
            spans.extend(parse_inline_spans(
                rest,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // H2
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let mut spans = vec![Span::styled(
                "▌ ".to_string(),
                Style::default().fg(Color::Rgb(100, 160, 220)),
            )];
            spans.extend(parse_inline_spans(
                rest,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // H1
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let mut spans = vec![Span::styled(
                "▌ ".to_string(),
                Style::default().fg(Color::Magenta),
            )];
            spans.extend(parse_inline_spans(
                rest,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let mut spans = vec![Span::styled(
                "│ ".to_string(),
                Style::default().fg(Color::DarkGray),
            )];
            spans.extend(parse_inline_spans(
                rest,
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::ITALIC),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // Unordered list item
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let indent = raw.len() - trimmed.len();
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled("• ".to_string(), Style::default().fg(Color::Cyan)),
            ];
            spans.extend(parse_inline_spans(
                rest,
                Style::default().fg(Color::White),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // Ordered list item (1. 2. etc.)
        if let Some((num, item_text)) = strip_ordered_list_prefix(trimmed) {
            let indent = raw.len() - trimmed.len();
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(format!("{num}. "), Style::default().fg(Color::Cyan)),
            ];
            spans.extend(parse_inline_spans(
                item_text,
                Style::default().fg(Color::White),
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // Normal text — apply inline spans
        lines.push(Line::from(parse_inline_spans(
            raw,
            Style::default().fg(Color::White),
        )));
    }

    if lines.is_empty() {
        lines.push(Line::from(String::new()));
    }

    lines
}

/// Returns `Some((number_str, rest_text))` for ordered list markers like `"1. "`.
fn strip_ordered_list_prefix(text: &str) -> Option<(&str, &str)> {
    let mut end = 0;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let rest = &text[end..];
    rest.strip_prefix(". ").map(|item| (&text[..end], item))
}

/// Parse inline markdown (`**bold**`, `*italic*`, `` `code` ``) into styled spans.
pub(crate) fn parse_inline_spans(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current = String::new();

    while i < len {
        // **bold**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
            }
            let start = i + 2;
            let mut j = start;
            while j + 1 < len && !(chars[j] == '*' && chars[j + 1] == '*') {
                j += 1;
            }
            if j + 1 < len {
                let bold_text: String = chars[start..j].iter().collect();
                spans.push(Span::styled(
                    bold_text,
                    base_style.add_modifier(Modifier::BOLD),
                ));
                i = j + 2;
            } else {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // *italic* (but not **)
        if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
            }
            let start = i + 1;
            let mut j = start;
            while j < len && chars[j] != '*' {
                j += 1;
            }
            if j < len {
                let italic_text: String = chars[start..j].iter().collect();
                spans.push(Span::styled(
                    italic_text,
                    base_style.add_modifier(Modifier::ITALIC),
                ));
                i = j + 1;
            } else {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // `inline code`
        if chars[i] == '`' {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), base_style));
            }
            let start = i + 1;
            let mut j = start;
            while j < len && chars[j] != '`' {
                j += 1;
            }
            if j < len {
                let code_text: String = chars[start..j].iter().collect();
                spans.push(Span::styled(
                    code_text,
                    Style::default()
                        .fg(Color::LightGreen)
                        .bg(Color::Rgb(40, 44, 52)),
                ));
                i = j + 1;
            } else {
                current.push(chars[i]);
                i += 1;
            }
            continue;
        }

        current.push(chars[i]);
        i += 1;
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, base_style));
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base_style));
    }

    spans
}
