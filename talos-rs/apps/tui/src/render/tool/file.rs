use talos_sdk::TalosContentItem;

use crate::render::tool::{helpers, tool_title};
use crate::types::{ChatBlock, ToolCallStatus, ToolRenderData};

pub(crate) fn build_file_render_data(
    block: &ChatBlock,
    items: &[TalosContentItem],
) -> ToolRenderData {
    let mut total_lines = 0usize;
    let mut truncated = false;
    let mut detail_lines = Vec::new();
    let mut output_lines = Vec::new();

    for item in items {
        match item {
            TalosContentItem::FileRead {
                path,
                offset,
                limit,
                total_lines: item_total_lines,
                content,
                truncated: item_truncated,
            } => {
                let start = offset.saturating_add(1);
                let end = offset.saturating_add(*limit).min(*item_total_lines);
                detail_lines.push(format!("{path}:{start}-{end}"));
                total_lines += content.lines().count();
                truncated |= *item_truncated;
                for (index, line) in content.lines().enumerate() {
                    output_lines.push(format!("{:>4} {}", start + index, line));
                }
            }
            TalosContentItem::Text { text } => {
                total_lines += text.lines().count();
                output_lines.extend(text.lines().map(str::to_string));
            }
            _ => {}
        }
    }

    let summary = match block.tool_status {
        Some(ToolCallStatus::Queued) => "Queued...".to_string(),
        Some(ToolCallStatus::Running) => "Reading...".to_string(),
        Some(ToolCallStatus::PendingDynamicInput) => "Input needed".to_string(),
        Some(ToolCallStatus::Completed { exit_ok: false }) => "read failed".to_string(),
        Some(ToolCallStatus::Completed { exit_ok: true }) | None => {
            let suffix = if truncated { " · truncated" } else { "" };
            format!("Read {total_lines} lines{suffix}")
        }
    };
    let preview_lines = output_lines
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| vec![helpers::compact_preview_text(line)])
        .unwrap_or_default();

    ToolRenderData {
        title: tool_title(block, block.tool_name.as_deref().unwrap_or("Read")),
        summary,
        detail_lines,
        preview_lines,
        output_lines,
        force_show_output: false,
    }
}

pub(crate) fn build_read_render_data(block: &ChatBlock) -> ToolRenderData {
    let parsed = helpers::parse_tool_output_json(&block.content);
    let primary_text = parsed
        .as_ref()
        .and_then(|value| value.get("result"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|value| value.get("error"))
                .and_then(|value| value.as_str())
        })
        .unwrap_or(block.content.as_str());
    let line_count = primary_text.lines().count();
    let summary = helpers::status_summary(block, |_| format!("Read {line_count} lines"));
    let output_lines = helpers::tool_output_lines(&block.content);
    let preview_lines = output_lines
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| vec![helpers::compact_preview_text(line)])
        .unwrap_or_default();

    ToolRenderData {
        title: tool_title(block, block.tool_name.as_deref().unwrap_or("Read")),
        summary,
        detail_lines: Vec::new(),
        preview_lines,
        output_lines,
        force_show_output: false,
    }
}
