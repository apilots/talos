use talos_sdk::TalosContentItem;

use crate::render::tool::{helpers, tool_title};
use crate::types::{ChatBlock, ToolCallStatus, ToolRenderData};

pub(crate) fn build_explored_render_data(
    block: &ChatBlock,
    items: &[TalosContentItem],
) -> ToolRenderData {
    let mut matches_count = 0usize;
    let mut more_available = false;
    let mut partial = false;
    let mut output_lines = Vec::new();

    for item in items {
        if let TalosContentItem::SearchResults {
            matches,
            more_available: item_more_available,
            partial: item_partial,
            total_matches,
            ..
        } = item
        {
            matches_count += total_matches.unwrap_or(matches.len());
            more_available |= *item_more_available;
            partial |= *item_partial;
            for item_match in matches {
                output_lines.push(format!(
                    "{}:{} {}",
                    item_match.path, item_match.line, item_match.text
                ));
                for before in item_match.before.iter().take(2) {
                    output_lines.push(format!("  {before}"));
                }
                for after in item_match.after.iter().take(2) {
                    output_lines.push(format!("  {after}"));
                }
            }
        }
    }

    let marker = if partial {
        " · partial"
    } else if more_available {
        " · more"
    } else {
        ""
    };
    let summary = match block.tool_status {
        Some(ToolCallStatus::Queued) => "Queued...".to_string(),
        Some(ToolCallStatus::Running) => "Searching...".to_string(),
        Some(ToolCallStatus::PendingDynamicInput) => "Input needed".to_string(),
        Some(ToolCallStatus::Completed { exit_ok: false }) => "search failed".to_string(),
        Some(ToolCallStatus::Completed { exit_ok: true }) | None => {
            format!("Found {matches_count} matches{marker}")
        }
    };

    ToolRenderData {
        title: tool_title(block, block.tool_name.as_deref().unwrap_or("Grep")),
        summary,
        detail_lines: Vec::new(),
        preview_lines: preview_lines(&output_lines),
        output_lines,
        force_show_output: false,
    }
}

pub(crate) fn build_glob_render_data(block: &ChatBlock) -> ToolRenderData {
    let parsed = helpers::parse_tool_output_json(&block.content);
    let entries = parsed
        .as_ref()
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let output_lines = if entries.is_empty() {
        helpers::tool_output_lines(&block.content)
    } else {
        entries.clone()
    };
    let summary = helpers::status_summary(block, |_| format!("Found {} paths", output_lines.len()));

    ToolRenderData {
        title: tool_title(block, "Glob"),
        summary,
        detail_lines: Vec::new(),
        preview_lines: preview_lines(&output_lines),
        output_lines,
        force_show_output: false,
    }
}

pub(crate) fn build_grep_render_data(block: &ChatBlock) -> ToolRenderData {
    let parsed = helpers::parse_tool_output_json(&block.content);
    let content = parsed
        .as_ref()
        .and_then(|value| value.get("content"))
        .and_then(|value| value.as_str())
        .unwrap_or(block.content.as_str());
    let matches_found = parsed
        .as_ref()
        .and_then(|value| value.get("matches_found"))
        .and_then(|value| value.as_u64())
        .unwrap_or_else(|| {
            content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as u64
        });
    let output_lines = helpers::tool_output_lines(content);
    let summary = helpers::status_summary(block, |_| format!("Found {matches_found} matches"));

    ToolRenderData {
        title: tool_title(block, "Grep"),
        summary,
        detail_lines: Vec::new(),
        preview_lines: preview_lines(&output_lines),
        output_lines,
        force_show_output: false,
    }
}

fn preview_lines(output_lines: &[String]) -> Vec<String> {
    output_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .map(|line| helpers::compact_preview_text(line))
        .collect()
}
