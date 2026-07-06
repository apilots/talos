use talos_sdk::TalosContentItem;
use talos_types::talos::TalosToolDiffLine;

use crate::types::{ChatBlock, ToolCallStatus, ToolRenderData};

pub(crate) fn build_diff_render_data(
    block: &ChatBlock,
    items: &[TalosContentItem],
) -> ToolRenderData {
    let mut file_count = 0usize;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut truncated = false;
    let mut output_lines = Vec::new();

    for item in items {
        let TalosContentItem::Diff { files } = item else {
            continue;
        };
        file_count += files.len();
        for file in files {
            let (file_added, file_removed) = file_change_counts(file);
            added += file_added;
            removed += file_removed;
            truncated |= file.truncated;
            output_lines.push(format!("{} (+{} -{})", file.path, file_added, file_removed));
            if let Some(old_path) = &file.old_path {
                output_lines.push(format!("rename from {old_path}"));
                output_lines.push(format!("rename to {}", file.path));
            }
            for hunk in &file.hunks {
                output_lines.push(format!(
                    "@@ -{},{} +{},{} @@",
                    hunk.old_start.unwrap_or(0),
                    hunk.old_lines.unwrap_or(0),
                    hunk.new_start.unwrap_or(0),
                    hunk.new_lines.unwrap_or(0)
                ));
                for line in &hunk.lines {
                    match line {
                        TalosToolDiffLine::Context { text } => {
                            output_lines.push(format!(" {text}"))
                        }
                        TalosToolDiffLine::Added { text } => output_lines.push(format!("+{text}")),
                        TalosToolDiffLine::Removed { text } => {
                            output_lines.push(format!("-{text}"))
                        }
                    }
                }
            }
            output_lines.push(String::new());
        }
    }
    while output_lines.last().is_some_and(|line| line.is_empty()) {
        output_lines.pop();
    }

    let title = format!("Edited {file_count} files (+{added} -{removed})");
    let summary = match block.tool_status {
        Some(ToolCallStatus::Queued) => "Queued...".to_string(),
        Some(ToolCallStatus::Running) => "Editing files...".to_string(),
        Some(ToolCallStatus::PendingDynamicInput) => "Input needed".to_string(),
        Some(ToolCallStatus::Completed { exit_ok: false }) => "edit failed".to_string(),
        Some(ToolCallStatus::Completed { exit_ok: true }) | None => {
            if truncated {
                format!("{file_count} files changed · diff truncated")
            } else {
                format!("{file_count} files changed")
            }
        }
    };

    ToolRenderData {
        title,
        summary,
        detail_lines: Vec::new(),
        preview_lines: Vec::new(),
        output_lines,
        force_show_output: true,
    }
}

fn file_change_counts(file: &talos_types::talos::TalosToolFileDiff) -> (usize, usize) {
    let mut added = 0usize;
    let mut removed = 0usize;
    for hunk in &file.hunks {
        for line in &hunk.lines {
            match line {
                TalosToolDiffLine::Added { .. } => added += 1,
                TalosToolDiffLine::Removed { .. } => removed += 1,
                TalosToolDiffLine::Context { .. } => {}
            }
        }
    }
    (added, removed)
}
