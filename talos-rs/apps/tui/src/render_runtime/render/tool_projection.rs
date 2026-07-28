//! Tool transcript projection for finalized tool results.

use crate::render_runtime::common::style::{StyledLine, StyledSegment, TextColor, TextStyle};
use crate::render_runtime::render::tool_copy::tool_copy_markdown;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use crate::utils::display_tool_name;
use talos_sdk::TalosContentItem;
use talos_types::talos::{TalosToolDiffLine, TalosToolTaskStatus};

use super::subagent_projection::project_subagent;

const TOOL_PREVIEW_LIMIT: usize = 96;

pub(crate) fn tool_running_line(name: &str, call_id: &str, arguments: &str) -> StyledLine {
    tool_header(format!("{} running", tool_title(name, call_id, arguments)))
}

pub(crate) fn tool_completed_block(
    name: &str,
    call_id: &str,
    arguments: &str,
    is_error: bool,
    content: &[TalosContentItem],
) -> FinalizedTranscriptBlock {
    let projection = project_tool_content(name, call_id, arguments, content, is_error);
    let mut lines = vec![tool_header(projection.title.clone())];
    push_projected_lines(&mut lines, projection);
    FinalizedTranscriptBlock::new(TranscriptBlockKind::Tool, lines)
        .with_copy_markdown(tool_copy_markdown(name, arguments, content))
}

fn tool_header(text: String) -> StyledLine {
    StyledLine::from_segments(vec![StyledSegment::new(
        format!("● {text}"),
        TextStyle::foreground(TextColor::Green).bold(),
    )])
}

#[derive(Default)]
pub(super) struct ToolRenderData {
    pub(super) title: String,
    pub(super) summary: String,
    pub(super) detail_lines: Vec<String>,
    pub(super) preview_lines: Vec<String>,
    pub(super) output_lines: Vec<String>,
    pub(super) force_show_output: bool,
}

fn project_tool_content(
    name: &str,
    call_id: &str,
    arguments: &str,
    content: &[TalosContentItem],
    is_error: bool,
) -> ToolRenderData {
    if let Some(projection) = project_subagent(name, call_id, arguments, content, is_error) {
        return projection;
    }
    if let Some(projection) = project_diff(content, is_error) {
        return projection;
    }
    if let Some(projection) = project_bash(name, call_id, arguments, content, is_error) {
        return projection;
    }
    if let Some(projection) = project_file_read(name, call_id, arguments, content, is_error) {
        return projection;
    }
    if let Some(projection) = project_search(name, call_id, arguments, content, is_error) {
        return projection;
    }
    if let Some(projection) = project_reference(name, call_id, arguments, content, is_error) {
        return projection;
    }
    project_generic(name, call_id, arguments, content, is_error)
}

fn push_projected_lines(lines: &mut Vec<StyledLine>, projection: ToolRenderData) {
    if projection.force_show_output {
        if !projection.summary.is_empty() {
            lines.push(StyledLine::plain(format!("  ⎿ {}", projection.summary)));
        }
        for output in projection.output_lines {
            lines.push(StyledLine::plain(format!("  │ {output}")));
        }
        return;
    }

    let has_extra = !projection.detail_lines.is_empty()
        || !projection.preview_lines.is_empty()
        || !projection.output_lines.is_empty();
    let mut summary = projection.summary;
    if has_extra {
        summary.push_str(" (ctrl+o to expand)");
    }
    if !summary.is_empty() {
        lines.push(StyledLine::plain(format!("  ⎿ {summary}")));
    }
}

fn project_bash(
    name: &str,
    call_id: &str,
    arguments: &str,
    content: &[TalosContentItem],
    is_error: bool,
) -> Option<ToolRenderData> {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut status = None;
    let mut exit_code = None;
    let mut elapsed_ms = None;
    let mut truncated = false;
    for item in content {
        if let TalosContentItem::BashOutput {
            status: item_status,
            exit_code: item_exit_code,
            stdout: item_stdout,
            stderr: item_stderr,
            truncated: item_truncated,
            elapsed_ms: item_elapsed_ms,
            ..
        } = item
        {
            append_output(&mut stdout, item_stdout);
            append_output(&mut stderr, item_stderr);
            status = Some(*item_status);
            exit_code = *item_exit_code;
            elapsed_ms = *item_elapsed_ms;
            truncated |= *item_truncated;
        }
    }
    status.map(|status| {
        let timing = timing_suffix(elapsed_ms);
        let mut summary = bash_summary(is_error, exit_code, &stdout, &stderr, status, &timing);
        if truncated {
            summary.push_str(" · truncated");
        }
        let mut preview_lines = preview_lines(&stdout, &stderr);
        if truncated {
            preview_lines.push("output truncated".to_string());
        }
        ToolRenderData {
            title: completed_title(tool_title(name, call_id, arguments), is_error),
            summary,
            preview_lines,
            output_lines: combined_output_lines(&stdout, &stderr),
            force_show_output: false,
            ..ToolRenderData::default()
        }
    })
}

fn project_file_read(
    name: &str,
    call_id: &str,
    arguments: &str,
    content: &[TalosContentItem],
    is_error: bool,
) -> Option<ToolRenderData> {
    let mut total = 0usize;
    let mut detail_lines = Vec::new();
    let mut output_lines = Vec::new();
    let mut truncated = false;
    let mut has_file_read = false;
    for item in content {
        match item {
            TalosContentItem::FileRead {
                path,
                offset,
                limit,
                total_lines,
                content,
                truncated: item_truncated,
            } => {
                has_file_read = true;
                let start = offset.saturating_add(1);
                let end = offset.saturating_add(*limit).min(*total_lines);
                detail_lines.push(format!("{path}:{start}-{end}"));
                total += content.lines().count();
                truncated |= *item_truncated;
                output_lines.extend(
                    content
                        .lines()
                        .enumerate()
                        .map(|(index, line)| format!("{:>4} {line}", start + index)),
                );
            }
            TalosContentItem::Text { text } => {
                total += text.lines().count();
                output_lines.extend(text.lines().map(str::to_string));
            }
            _ => {}
        }
    }
    has_file_read.then(|| ToolRenderData {
        title: completed_title(tool_title(name, call_id, arguments), is_error),
        summary: if is_error {
            "read failed".to_string()
        } else {
            format!(
                "Read {total} lines{}",
                if truncated { " · truncated" } else { "" }
            )
        },
        detail_lines,
        preview_lines: preview_lines_from_output(&output_lines, 1),
        output_lines,
        force_show_output: false,
    })
}

fn project_search(
    name: &str,
    call_id: &str,
    arguments: &str,
    content: &[TalosContentItem],
    is_error: bool,
) -> Option<ToolRenderData> {
    let mut total = 0usize;
    let mut more = false;
    let mut partial = false;
    let mut output_lines = Vec::new();
    for item in content {
        if let TalosContentItem::SearchResults {
            matches,
            total_matches,
            more_available,
            partial: item_partial,
            ..
        } = item
        {
            total += total_matches.unwrap_or(matches.len());
            more |= *more_available;
            partial |= *item_partial;
            for item_match in matches {
                output_lines.push(format!(
                    "{}:{} {}",
                    item_match.path, item_match.line, item_match.text
                ));
                output_lines.extend(
                    item_match
                        .before
                        .iter()
                        .take(2)
                        .map(|line| format!("  {line}")),
                );
                output_lines.extend(
                    item_match
                        .after
                        .iter()
                        .take(2)
                        .map(|line| format!("  {line}")),
                );
            }
        }
    }
    (total > 0 || !output_lines.is_empty()).then(|| {
        let marker = if partial {
            " · partial"
        } else if more {
            " · more"
        } else {
            ""
        };
        ToolRenderData {
            title: completed_title(tool_title(name, call_id, arguments), is_error),
            summary: if is_error {
                "search failed".to_string()
            } else {
                format!("Found {total} matches{marker}")
            },
            preview_lines: preview_lines_from_output(&output_lines, 2),
            output_lines,
            force_show_output: false,
            ..ToolRenderData::default()
        }
    })
}

fn project_diff(content: &[TalosContentItem], is_error: bool) -> Option<ToolRenderData> {
    let mut file_count = 0usize;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut truncated = false;
    let mut output_lines = Vec::new();
    for item in content {
        if let TalosContentItem::Diff { files: item_files } = item {
            file_count += item_files.len();
            for file in item_files {
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
                            TalosToolDiffLine::Added { text } => {
                                output_lines.push(format!("+{text}"))
                            }
                            TalosToolDiffLine::Removed { text } => {
                                output_lines.push(format!("-{text}"))
                            }
                        }
                    }
                }
                output_lines.push(String::new());
            }
        }
    }
    while output_lines.last().is_some_and(|line| line.is_empty()) {
        output_lines.pop();
    }
    (file_count > 0).then(|| ToolRenderData {
        title: format!("Edited {file_count} files (+{added} -{removed})"),
        summary: if is_error {
            "edit failed".to_string()
        } else if truncated {
            "diff truncated".to_string()
        } else {
            String::new()
        },
        output_lines,
        force_show_output: true,
        ..ToolRenderData::default()
    })
}

fn project_reference(
    name: &str,
    call_id: &str,
    arguments: &str,
    content: &[TalosContentItem],
    is_error: bool,
) -> Option<ToolRenderData> {
    let mut output_lines = Vec::new();
    let mut has_reference = false;
    for item in content {
        match item {
            TalosContentItem::WebReference {
                url,
                title,
                snippet,
                source_domain,
                ..
            } => {
                has_reference = true;
                output_lines.push(title.clone().unwrap_or_else(|| url.clone()));
                output_lines.push(format!("  {}", source_domain.as_deref().unwrap_or(url)));
                if let Some(snippet) = snippet {
                    output_lines.push(format!("  {snippet}"));
                }
            }
            TalosContentItem::ArtifactRef {
                artifact_id,
                mime_type,
                label,
            } => {
                has_reference = true;
                output_lines.push(format!(
                    "artifact: {}{}",
                    label.as_deref().unwrap_or(artifact_id),
                    mime_type
                        .as_ref()
                        .map(|mime| format!(" · {mime}"))
                        .unwrap_or_default()
                ));
            }
            TalosContentItem::McpResourceRef {
                server,
                uri,
                mime_type,
            } => {
                has_reference = true;
                output_lines.push(format!(
                    "mcp: {server}:{uri}{}",
                    mime_type
                        .as_ref()
                        .map(|mime| format!(" · {mime}"))
                        .unwrap_or_default()
                ));
            }
            TalosContentItem::Text { text } => {
                output_lines.extend(text.lines().map(str::to_string));
            }
            _ => {}
        }
    }
    let count = output_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .count();
    has_reference.then(|| ToolRenderData {
        title: completed_title(tool_title(name, call_id, arguments), is_error),
        summary: if is_error {
            "reference failed".to_string()
        } else {
            format!("{count} references")
        },
        preview_lines: preview_lines_from_output(&output_lines, 2),
        output_lines,
        force_show_output: false,
        ..ToolRenderData::default()
    })
}

fn project_generic(
    name: &str,
    call_id: &str,
    arguments: &str,
    content: &[TalosContentItem],
    is_error: bool,
) -> ToolRenderData {
    let output_lines = content
        .iter()
        .flat_map(|item| {
            item.display_text()
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let summary = if is_error {
        "error".to_string()
    } else {
        first_non_empty_line(output_lines.iter().map(String::as_str))
            .map(|line| compact_tool_preview(&line))
            .unwrap_or_else(|| "Done".to_string())
    };
    ToolRenderData {
        title: completed_title(tool_title(name, call_id, arguments), is_error),
        summary,
        preview_lines: preview_lines_from_output(&output_lines, 2),
        output_lines,
        force_show_output: false,
        ..ToolRenderData::default()
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

pub(super) fn tool_title(name: &str, _call_id: &str, arguments: &str) -> String {
    let name = display_tool_name(name);
    tool_title_argument(name, arguments)
        .map(|argument| format!("{name}({})", compact_tool_preview(&argument)))
        .unwrap_or_else(|| name.to_string())
}

fn tool_title_argument(name: &str, arguments: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(arguments).ok()?;
    let key = match name {
        "Bash" => value
            .get("cmd")
            .or_else(|| value.get("command"))
            .and_then(|value| value.as_str()),
        "Read" | "Write" | "Edit" | "ApplyPatch" => value
            .get("file_path")
            .or_else(|| value.get("path"))
            .or_else(|| value.get("patch"))
            .and_then(|value| value.as_str()),
        "Grep" => value
            .get("pattern")
            .or_else(|| value.get("query"))
            .and_then(|value| value.as_str()),
        "Glob" => value
            .get("pattern")
            .or_else(|| value.get("glob"))
            .and_then(|value| value.as_str()),
        _ => value.as_object().and_then(|object| {
            object
                .values()
                .find_map(|value| value.as_str().filter(|text| !text.is_empty()))
        }),
    }?;
    Some(key.to_string())
}

fn append_output(target: &mut String, value: &str) {
    if value.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(value);
}

pub(super) fn completed_title(title: String, is_error: bool) -> String {
    format!("{title} {}", if is_error { "failed" } else { "completed" })
}

fn timing_suffix(timing_ms: Option<u64>) -> String {
    timing_ms
        .map(|ms| format!(" · {:.1}s", ms as f64 / 1000.0))
        .unwrap_or_default()
}

fn bash_summary(
    is_error: bool,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    status: TalosToolTaskStatus,
    timing: &str,
) -> String {
    if is_error || matches!(status, TalosToolTaskStatus::Failed) {
        let detail = first_non_empty_line(stderr.lines().chain(stdout.lines()))
            .map(|line| format!(" {}", compact_tool_preview(&line)))
            .unwrap_or_default();
        return exit_code
            .map(|code| format!("exit {code}{detail}{timing}"))
            .unwrap_or_else(|| format!("Error{detail}{timing}"));
    }

    if let Some(line) = first_non_empty_line(stdout.lines().chain(stderr.lines())) {
        let status_text = task_status_text(status).trim_end_matches('.');
        if matches!(status, TalosToolTaskStatus::Completed) {
            format!("Done {}{timing}", compact_tool_preview(&line))
        } else {
            format!("{status_text} {}{timing}", compact_tool_preview(&line))
        }
    } else {
        format!("{}{timing}", task_status_text(status))
    }
}

fn preview_lines(stdout: &str, stderr: &str) -> Vec<String> {
    stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| !line.trim().is_empty())
        .take(2)
        .map(compact_tool_preview)
        .collect()
}

pub(super) fn preview_lines_from_output(output_lines: &[String], limit: usize) -> Vec<String> {
    output_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(limit)
        .map(|line| compact_tool_preview(line))
        .collect()
}

fn combined_output_lines(stdout: &str, stderr: &str) -> Vec<String> {
    let mut output_lines = stdout.lines().map(str::to_string).collect::<Vec<_>>();
    if !stderr.trim().is_empty() {
        if !output_lines.is_empty() {
            output_lines.push(String::new());
        }
        output_lines.push("stderr:".to_string());
        output_lines.extend(stderr.lines().map(str::to_string));
    }
    output_lines
}

fn first_non_empty_line<'a>(lines: impl Iterator<Item = &'a str>) -> Option<String> {
    lines
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

pub(crate) fn compact_tool_preview(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let preview = chars.by_ref().take(TOOL_PREVIEW_LIMIT).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn task_status_text(status: TalosToolTaskStatus) -> &'static str {
    match status {
        TalosToolTaskStatus::Queued => "Queued...",
        TalosToolTaskStatus::Running => "Running...",
        TalosToolTaskStatus::Completed => "Done",
        TalosToolTaskStatus::Failed => "Error",
        TalosToolTaskStatus::TimedOut => "Timed out",
        TalosToolTaskStatus::Cancelled => "Cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::tool_completed_block;
    use talos_sdk::TalosContentItem;
    use talos_types::talos::{
        TalosToolDiffHunk, TalosToolDiffLine, TalosToolFileDiff, TalosToolTaskStatus,
    };

    #[test]
    fn projects_bash_output_as_summary_and_detail_lines() {
        let block = tool_completed_block(
            "bash",
            "call-1",
            r#"{"cmd":"printf ok"}"#,
            false,
            &[TalosContentItem::BashOutput {
                session_id: None,
                status: TalosToolTaskStatus::Completed,
                exit_code: Some(0),
                stdout: "ok\nnext".to_string(),
                stderr: String::new(),
                truncated: false,
                elapsed_ms: Some(12),
            }],
        );

        let lines = block
            .lines
            .iter()
            .map(crate::render_runtime::common::style::StyledLine::plain_text)
            .collect::<Vec<_>>();
        assert_eq!(
            lines,
            vec![
                "● Bash(printf ok) completed".to_string(),
                "  ⎿ Done ok · 0.0s (ctrl+o to expand)".to_string(),
            ]
        );
    }

    #[test]
    fn projects_file_read_with_path_and_line_preview() {
        let block = tool_completed_block(
            "Read",
            "call-2",
            r#"{"file_path":"/tmp/demo.rs"}"#,
            false,
            &[TalosContentItem::FileRead {
                path: "/tmp/demo.rs".to_string(),
                offset: 10,
                limit: 2,
                total_lines: 20,
                content: "fn main() {}\n".to_string(),
                truncated: false,
            }],
        );

        let lines = block
            .lines
            .iter()
            .map(crate::render_runtime::common::style::StyledLine::plain_text)
            .collect::<Vec<_>>();
        assert_eq!(
            lines,
            vec![
                "● Read(/tmp/demo.rs) completed".to_string(),
                "  ⎿ Read 1 lines (ctrl+o to expand)".to_string(),
            ]
        );
    }

    #[test]
    fn projects_diff_as_edited_output_without_detail_folding() {
        let block = tool_completed_block(
            "Edit",
            "call-3",
            r#"{"file_path":"/tmp/demo.rs"}"#,
            false,
            &[TalosContentItem::Diff {
                files: vec![TalosToolFileDiff {
                    path: "/tmp/demo.rs".to_string(),
                    old_path: None,
                    hunks: vec![TalosToolDiffHunk {
                        old_start: Some(1),
                        old_lines: Some(2),
                        new_start: Some(1),
                        new_lines: Some(2),
                        lines: vec![
                            TalosToolDiffLine::Context {
                                text: "fn main() {".to_string(),
                            },
                            TalosToolDiffLine::Removed {
                                text: "    old();".to_string(),
                            },
                            TalosToolDiffLine::Added {
                                text: "    new();".to_string(),
                            },
                        ],
                    }],
                    truncated: false,
                }],
            }],
        );

        let lines = block
            .lines
            .iter()
            .map(crate::render_runtime::common::style::StyledLine::plain_text)
            .collect::<Vec<_>>();
        assert_eq!(
            lines,
            vec![
                "● Edited 1 files (+1 -1)".to_string(),
                "  │ /tmp/demo.rs (+1 -1)".to_string(),
                "  │ @@ -1,2 +1,2 @@".to_string(),
                "  │  fn main() {".to_string(),
                "  │ -    old();".to_string(),
                "  │ +    new();".to_string(),
            ]
        );
    }

    #[test]
    fn projects_diff_content_before_other_tool_content() {
        let block = tool_completed_block(
            "apply_patch",
            "call-4",
            r#"{"patch":"*** Begin Patch"}"#,
            false,
            &[
                TalosContentItem::FileRead {
                    path: "/tmp/demo.rs".to_string(),
                    offset: 0,
                    limit: 1,
                    total_lines: 1,
                    content: "old();".to_string(),
                    truncated: false,
                },
                TalosContentItem::Diff {
                    files: vec![TalosToolFileDiff {
                        path: "/tmp/demo.rs".to_string(),
                        old_path: None,
                        hunks: vec![TalosToolDiffHunk {
                            old_start: Some(1),
                            old_lines: Some(1),
                            new_start: Some(1),
                            new_lines: Some(1),
                            lines: vec![TalosToolDiffLine::Added {
                                text: "new();".to_string(),
                            }],
                        }],
                        truncated: false,
                    }],
                },
            ],
        );

        let lines = block
            .lines
            .iter()
            .map(crate::render_runtime::common::style::StyledLine::plain_text)
            .collect::<Vec<_>>();
        assert_eq!(lines[0], "● Edited 1 files (+1 -0)");
        assert!(lines.iter().any(|line| line == "  │ +new();"));
        assert!(!lines.iter().any(|line| line.contains("ApplyPatch")));
        assert!(!lines.iter().any(|line| line.starts_with("  ⎿ Read")));
    }
}
