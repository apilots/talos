use talos_types::talos::TalosToolTaskStatus;

use crate::types::{ChatBlock, ToolCallStatus};
use crate::TOOL_PREVIEW_CHAR_LIMIT;

pub(crate) fn compact_inline(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

pub(crate) fn compact_preview_text(text: &str) -> String {
    compact_inline(text, TOOL_PREVIEW_CHAR_LIMIT)
}

pub(crate) fn timing_suffix(timing_ms: Option<u64>) -> String {
    timing_ms
        .map(|ms| format!(" · {:.1}s", ms as f64 / 1000.0))
        .unwrap_or_default()
}

pub(crate) fn status_summary(
    block: &ChatBlock,
    completed_summary: impl FnOnce(String) -> String,
) -> String {
    match block.tool_status {
        Some(ToolCallStatus::Queued) => "Queued...".to_string(),
        Some(ToolCallStatus::Running) => "Running...".to_string(),
        Some(ToolCallStatus::PendingDynamicInput) => "Input needed".to_string(),
        Some(ToolCallStatus::Completed { .. }) | None => {
            completed_summary(timing_suffix(block.timing_ms))
        }
    }
}

pub(crate) fn task_status_text(status: TalosToolTaskStatus) -> &'static str {
    match status {
        TalosToolTaskStatus::Queued => "Queued...",
        TalosToolTaskStatus::Running => "Running...",
        TalosToolTaskStatus::Completed => "Done",
        TalosToolTaskStatus::Failed => "Error",
        TalosToolTaskStatus::TimedOut => "Timed out",
        TalosToolTaskStatus::Cancelled => "Cancelled",
    }
}

pub(crate) fn json_lines(value: &serde_json::Value) -> Vec<String> {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| value.to_string())
        .lines()
        .map(str::to_string)
        .collect()
}

pub(crate) fn parse_tool_output_json(content: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(content).ok()
}

pub(crate) fn tool_output_lines(content: &str) -> Vec<String> {
    if let Some(value) = parse_tool_output_json(content) {
        json_lines(&value)
    } else {
        content.lines().map(str::to_string).collect()
    }
}

pub(crate) fn first_non_empty_line<'a>(lines: impl Iterator<Item = &'a str>) -> Option<String> {
    lines
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}
