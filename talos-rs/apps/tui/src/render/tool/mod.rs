pub(crate) mod bash;
pub(crate) mod diff;
pub(crate) mod file;
pub(crate) mod generic;
pub(crate) mod helpers;
pub(crate) mod reference;
pub(crate) mod search;

use talos_sdk::TalosContentItem;

use crate::types::{ChatBlock, ToolCallStatus, ToolRenderData};
use crate::utils::display_tool_name;

pub(crate) fn build_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData {
    let mut data = if !block.tool_content_items.is_empty() {
        build_typed_tool_render_data(block, tool_name)
    } else {
        match tool_name {
            "MemorySearch" | "memory_search" => generic::build_memory_search_render_data(block),
            "bash" | "Bash" => bash::build_bash_render_data(block),
            "read" | "Read" | "write_file" | "Write" => file::build_read_render_data(block),
            "glob" | "Glob" => search::build_glob_render_data(block),
            "grep" | "Grep" => search::build_grep_render_data(block),
            _ => generic::build_generic_tool_render_data(block, tool_name),
        }
    };
    apply_live_progress(block, &mut data);
    data
}

fn build_typed_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData {
    let items = &block.tool_content_items;
    if items
        .iter()
        .any(|item| matches!(item, TalosContentItem::BashOutput { .. }))
    {
        return bash::build_task_render_data(block, items);
    }
    if items
        .iter()
        .any(|item| matches!(item, TalosContentItem::Diff { .. }))
    {
        return diff::build_diff_render_data(block, items);
    }
    if items
        .iter()
        .any(|item| matches!(item, TalosContentItem::FileRead { .. }))
    {
        return file::build_file_render_data(block, items);
    }
    if items
        .iter()
        .any(|item| matches!(item, TalosContentItem::SearchResults { .. }))
    {
        return search::build_explored_render_data(block, items);
    }
    if items.iter().any(|item| {
        matches!(
            item,
            TalosContentItem::WebReference { .. }
                | TalosContentItem::ArtifactRef { .. }
                | TalosContentItem::McpResourceRef { .. }
        )
    }) {
        return reference::build_reference_render_data(block, items);
    }
    generic::build_text_json_render_data(block, items, tool_name)
}

pub(crate) fn tool_title(block: &ChatBlock, tool_name: &str) -> String {
    let name = display_tool_name(tool_name);
    let Some(input) = &block.tool_input else {
        return name.to_string();
    };
    let value = input.as_inner();
    let arg = match tool_name {
        "bash" | "Bash" => value
            .get("cmd")
            .or_else(|| value.get("command"))
            .and_then(|value| value.as_str()),
        "read" | "Read" | "write_file" | "Write" => value
            .get("file_path")
            .or_else(|| value.get("path"))
            .and_then(|value| value.as_str()),
        "grep" | "Grep" => value
            .get("pattern")
            .or_else(|| value.get("query"))
            .and_then(|value| value.as_str()),
        "glob" | "Glob" => value.get("pattern").and_then(|value| value.as_str()),
        "web_search" | "WebSearch" => value.get("query").and_then(|value| value.as_str()),
        "web_fetch" | "WebFetch" => value.get("url").and_then(|value| value.as_str()),
        _ => value.as_object().and_then(|object| {
            object
                .values()
                .find_map(|value| value.as_str().filter(|text| !text.is_empty()))
        }),
    };

    arg.map(|arg| format!("{name}({})", helpers::compact_inline(arg, 96)))
        .unwrap_or_else(|| name.to_string())
}

fn apply_live_progress(block: &ChatBlock, data: &mut ToolRenderData) {
    if !matches!(block.tool_status, Some(ToolCallStatus::Running)) {
        return;
    }
    let timing = helpers::timing_suffix(block.timing_ms);
    let progress = block
        .tool_progress
        .last()
        .map(|line| helpers::compact_preview_text(line))
        .unwrap_or_default();
    data.summary = if progress.is_empty() {
        format!("Running...{timing}")
    } else {
        format!("Running... {progress}{timing}")
    };
    if data.preview_lines.is_empty() {
        data.preview_lines = block.tool_progress.clone();
    }
}
