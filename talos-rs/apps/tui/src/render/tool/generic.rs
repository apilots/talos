use talos_sdk::TalosContentItem;

use crate::render::tool::{helpers, tool_title};
use crate::types::{ChatBlock, ToolRenderData};

pub(crate) fn build_text_json_render_data(
    block: &ChatBlock,
    items: &[TalosContentItem],
    tool_name: &str,
) -> ToolRenderData {
    let mut output_lines = Vec::new();
    for item in items {
        match item {
            TalosContentItem::Text { text } => {
                output_lines.extend(text.lines().map(str::to_string))
            }
            TalosContentItem::Json { value } => {
                output_lines.extend(helpers::json_lines(value.as_inner()))
            }
            TalosContentItem::Progress { message, .. } => output_lines.push(message.clone()),
            other => output_lines.extend(other.display_text().lines().map(str::to_string)),
        }
    }
    text_render_data(block, tool_name, output_lines)
}

pub(crate) fn build_generic_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData {
    text_render_data(block, tool_name, helpers::tool_output_lines(&block.content))
}

pub(crate) fn build_memory_search_render_data(block: &ChatBlock) -> ToolRenderData {
    let parsed = serde_json::from_str::<serde_json::Value>(&block.content).ok();
    let results = parsed
        .as_ref()
        .and_then(|value| value.get("results"))
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut output_lines = Vec::new();
    for result in &results {
        if let Some(path) = result.get("path").and_then(|value| value.as_str()) {
            output_lines.push(format!("hit: {path}"));
        }
        if let Some(snippet) = result.get("snippet").and_then(|value| value.as_str()) {
            output_lines.extend(
                snippet
                    .lines()
                    .take(3)
                    .map(|line| format!("snippet: {line}")),
            );
        }
    }
    if output_lines.is_empty() {
        output_lines = helpers::tool_output_lines(&block.content);
    }

    ToolRenderData {
        title: tool_title(block, "MemorySearch"),
        summary: helpers::status_summary(block, |_| format!("Found {} hits", results.len())),
        detail_lines: Vec::new(),
        preview_lines: preview_lines(&output_lines),
        output_lines,
        force_show_output: false,
    }
}

fn text_render_data(
    block: &ChatBlock,
    tool_name: &str,
    output_lines: Vec<String>,
) -> ToolRenderData {
    ToolRenderData {
        title: tool_title(block, tool_name),
        summary: helpers::status_summary(block, |_| {
            helpers::first_non_empty_line(output_lines.iter().map(String::as_str))
                .map(|line| helpers::compact_preview_text(&line))
                .unwrap_or_else(|| "Done".to_string())
        }),
        detail_lines: generic_detail_lines(block),
        preview_lines: preview_lines(&output_lines),
        output_lines,
        force_show_output: false,
    }
}

fn generic_detail_lines(block: &ChatBlock) -> Vec<String> {
    block
        .tool_input
        .as_ref()
        .and_then(|input| {
            input.as_inner().as_object().and_then(|object| {
                object
                    .iter()
                    .find_map(|(key, value)| value.as_str().map(|value| format!("{key}: {value}")))
            })
        })
        .map(|line| vec![line])
        .unwrap_or_default()
}

fn preview_lines(output_lines: &[String]) -> Vec<String> {
    output_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .map(|line| helpers::compact_preview_text(line))
        .collect()
}
