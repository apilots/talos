//! Pure utility functions for TUI rendering and layout.

use ratatui::text::{Line, Span};
use std::{env, path::Path};
use unicode_width::UnicodeWidthChar;

pub(crate) const APP_VERSION: &str = "v0.1.9-SNAPSHOT";

// ── Event labels ────────────────────────────────────────────────────────────

pub fn display_tool_name(tool_name: &str) -> &str {
    match tool_name {
        "bash" | "Bash" => "Bash",
        "read" | "Read" => "Read",
        "glob" | "Glob" => "Glob",
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

pub(crate) fn build_welcome_card(
    version: &str,
    model: &str,
    working_directory: Option<&Path>,
    permission_mode: talos_sdk::UserPermissionMode,
) -> String {
    const CARD_WIDTH: usize = 50;

    fn card_line(content: &str) -> String {
        let mut inner: String = content.chars().take(CARD_WIDTH).collect();
        let used = inner.chars().count();
        if used < CARD_WIDTH {
            inner.push_str(&" ".repeat(CARD_WIDTH - used));
        }
        format!("│{}│", inner)
    }

    fn info_line(label: &str, value: &str) -> String {
        card_line(&format!(" {:<12} {}", label, value))
    }

    fn info_line_with_suffix(label: &str, value: &str, suffix: &str) -> String {
        let prefix = format!(" {:<12} {}", label, value);
        let prefix_len = prefix.chars().count();
        let suffix_len = suffix.chars().count();
        if prefix_len + suffix_len >= CARD_WIDTH {
            return card_line(&prefix);
        }
        card_line(&format!(
            "{}{}{}",
            prefix,
            " ".repeat(CARD_WIDTH - prefix_len - suffix_len),
            suffix
        ))
    }

    let directory = working_directory
        .map(compact_directory_path)
        .unwrap_or_else(|| "(server-managed)".to_string());
    [
        "╭──────────────────────────────────────────────────╮".to_string(),
        card_line(&format!(" >_ Talos TUI ({})", version)),
        card_line(""),
        info_line_with_suffix("model:", model, "/model to change"),
        info_line("directory:", &directory),
        info_line(
            "permissions:",
            crate::permission_picker::permission_mode_label(permission_mode),
        ),
        "╰──────────────────────────────────────────────────╯".to_string(),
    ]
    .join("\n")
}

#[cfg(test)]
#[path = "utils_tests.rs"]
mod tests;
