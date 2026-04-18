//! Pure utility functions for TUI rendering and layout.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::{
    env,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use talos_types::talos::TalosConnectionMode;
use unicode_width::UnicodeWidthChar;

pub(crate) const APP_VERSION: &str = "v0.1.0";

// ── Event labels ────────────────────────────────────────────────────────────

pub(crate) fn display_tool_name(tool_name: &str) -> &str {
    match tool_name {
        "bash" | "Bash" => "Bash",
        "read" | "Read" => "Read",
        "grep" | "Grep" => "Grep",
        "write" | "write_file" | "Write" => "Write",
        "edit" | "Edit" => "Edit",
        "memory_search" | "MemorySearch" => "MemorySearch",
        "memory_get" | "MemoryGet" => "MemoryGet",
        _ => tool_name,
    }
}

// ── Text / display width ─────────────────────────────────────────────────────

pub(crate) fn char_display_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

pub(crate) fn text_display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

pub(crate) fn line_plain_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

pub(crate) fn line_display_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .flat_map(|span| span.content.chars())
        .map(char_display_width)
        .sum()
}

pub(crate) fn byte_index_at_char(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or_else(|| text.len())
}

// ── Line wrapping ────────────────────────────────────────────────────────────

pub(crate) fn wrap_line_visual(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    let width_limit = max_width.max(1);
    if line.spans.is_empty() {
        return vec![Line::from(String::new())];
    }

    let mut wrapped_lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in line.spans {
        let style = span.style;
        let mut segment = String::new();
        let mut segment_width = 0usize;

        for ch in span.content.chars() {
            let ch_width = char_display_width(ch);
            let next_width = current_width + segment_width + ch_width;
            if next_width > width_limit && (current_width + segment_width) > 0 {
                if !segment.is_empty() {
                    current_spans.push(Span::styled(std::mem::take(&mut segment), style));
                    segment_width = 0;
                }
                wrapped_lines.push(Line::from(std::mem::take(&mut current_spans)));
                current_width = 0;
            }
            segment.push(ch);
            segment_width += ch_width;
        }

        if !segment.is_empty() {
            current_spans.push(Span::styled(segment, style));
            current_width += segment_width;
        }
    }

    if current_spans.is_empty() {
        wrapped_lines.push(Line::from(String::new()));
    } else {
        wrapped_lines.push(Line::from(current_spans));
    }
    wrapped_lines
}

// ── Selection / highlight ────────────────────────────────────────────────────

pub(crate) fn slice_text_by_display_columns(
    text: &str,
    start_col: usize,
    end_col: usize,
) -> String {
    if start_col >= end_col {
        return String::new();
    }
    let mut current_col = 0usize;
    let mut selected = String::new();
    for ch in text.chars() {
        let ch_width = char_display_width(ch);
        let next_col = current_col + ch_width;
        if next_col > start_col && current_col < end_col {
            selected.push(ch);
        }
        current_col = next_col;
        if current_col >= end_col {
            break;
        }
    }
    selected
}

pub(crate) fn highlight_line_columns(line: &mut Line<'static>, start_col: usize, end_col: usize) {
    let highlight_style = Style::default()
        .bg(Color::Rgb(108, 117, 160))
        .fg(Color::White);
    let mut current_col = 0usize;
    let mut spans = Vec::new();

    for span in &line.spans {
        for ch in span.content.chars() {
            let ch_width = char_display_width(ch);
            let next_col = current_col + ch_width;
            let selected = next_col > start_col && current_col < end_col;
            let style = if selected {
                span.style.patch(highlight_style)
            } else {
                span.style
            };
            push_styled_char(&mut spans, ch, style);
            current_col = next_col;
        }
    }

    line.spans = spans;
}

fn push_styled_char(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push(ch);
            return;
        }
    }
    spans.push(Span::styled(ch.to_string(), style));
}

// ── Display helpers ──────────────────────────────────────────────────────────

pub(crate) fn summarize_for_queue(input: &str) -> String {
    const MAX: usize = 42;
    let trimmed = input.trim();
    let mut chars = trimmed.chars();
    let head: String = chars.by_ref().take(MAX).collect();
    if chars.next().is_some() {
        format!("{}...", head)
    } else {
        head
    }
}

pub(crate) fn compact_directory_path(path: &Path) -> String {
    let raw = path.display().to_string();
    if let Ok(home) = env::var("HOME") {
        if raw.starts_with(&home) {
            return raw.replacen(&home, "~", 1);
        }
    }
    raw
}

pub(crate) fn copy_to_system_clipboard(text: &str) -> Result<(), String> {
    fn try_copy(command: &str, args: &[&str], text: &str) -> Result<(), String> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| e.to_string())?;
        }

        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{} exited with {}", command, status))
        }
    }

    let candidates: [(&str, &[&str]); 3] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
    ];
    for (cmd, args) in candidates {
        if try_copy(cmd, args, text).is_ok() {
            return Ok(());
        }
    }

    Err("no clipboard command available (pbcopy/wl-copy/xclip)".to_string())
}

pub(crate) fn build_welcome_card(
    version: &str,
    model: &str,
    working_directory: Option<&Path>,
    directory: &Path,
    connection_mode: TalosConnectionMode,
) -> String {
    const CARD_WIDTH: usize = 50;
    const INFO_BLOCK_INDENT: usize = 2;

    fn card_line(content: &str) -> String {
        let mut inner: String = content.chars().take(CARD_WIDTH).collect();
        let used = inner.chars().count();
        if used < CARD_WIDTH {
            inner.push_str(&" ".repeat(CARD_WIDTH - used));
        }
        format!("│{}│", inner)
    }

    fn indented_card_line(content: &str, indent: usize) -> String {
        card_line(&format!("{}{}", " ".repeat(indent), content))
    }

    let working_directory_line = working_directory
        .map(|path| format!("working_directory: {}", compact_directory_path(path)))
        .unwrap_or_else(|| "working_directory: (server-managed)".to_string());
    let directory_line = format!("client_directory: {}", compact_directory_path(directory));
    let connection_line = format!("connection: {}", connection_mode);
    [
        "╭──────────────────────────────────────────────────╮".to_string(),
        card_line(&format!(" >_ Talos TUI ({})", version)),
        card_line(""),
        indented_card_line("Session", INFO_BLOCK_INDENT),
        indented_card_line("", INFO_BLOCK_INDENT),
        indented_card_line(
            &format!("model:     {:<12} /model to change", model),
            INFO_BLOCK_INDENT,
        ),
        indented_card_line(&working_directory_line, INFO_BLOCK_INDENT),
        indented_card_line(&directory_line, INFO_BLOCK_INDENT),
        indented_card_line(&connection_line, INFO_BLOCK_INDENT),
        "╰──────────────────────────────────────────────────╯".to_string(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::build_welcome_card;
    use crate::utils::TalosConnectionMode;
    use std::path::Path;

    #[test]
    fn build_welcome_card_separates_title_and_session_block() {
        let card = build_welcome_card(
            "v0.1.0",
            "qwen-plus",
            Some(Path::new("/Users/xujiang/data/ailabs/talos/talos-rs")),
            Path::new("/Users/xujiang/data/ailabs/talos/talos-rs"),
            TalosConnectionMode::Embedded,
        );

        let lines: Vec<&str> = card.lines().collect();
        assert_eq!(
            lines[1],
            "│ >_ Talos TUI (v0.1.0)                            │"
        );
        assert_eq!(
            lines[3],
            "│  Session                                         │"
        );
        assert_eq!(
            lines[4],
            "│                                                  │"
        );
        assert!(lines[5].starts_with("│  model:"));
        assert!(lines[6].starts_with("│  working_directory:"));
        assert!(lines[7].starts_with("│  client_directory:"));
        assert!(lines[8].starts_with("│  connection:"));
    }
}
