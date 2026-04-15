use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use talos_types::talos::TalosToolProgressKind;

use crate::render::helpers::push_block_header_gap;
use crate::types::{ChatBlock, CopyProjection, RenderedChatLine, ToolCallStatus, ToolRenderData};
use crate::utils::{line_plain_text, wrap_line_visual};
use crate::TuiApp;
use crate::TOOL_PREVIEW_CHAR_LIMIT;

impl TuiApp {
    pub(crate) fn tool_progress_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let tool_name = block.tool_name.as_deref().unwrap_or("unknown");
        let (label, color) = match block.tool_progress_kind {
            Some(TalosToolProgressKind::Status) => ("⏳ Progress", Color::Yellow),
            Some(TalosToolProgressKind::Stdout) => ("↳ Stdout", Color::Cyan),
            Some(TalosToolProgressKind::Stderr) => ("↳ Stderr", Color::LightRed),
            None => ("⏳ Progress", Color::Gray),
        };

        let mut lines = vec![RenderedChatLine {
            display: Line::from(Span::styled(
                format!("{label} · {tool_name}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            copy_projection: Some(CopyProjection {
                text: format!("{label} · {tool_name}"),
                display_start_col: 0,
            }),
        }];
        push_block_header_gap(&mut lines);

        for line in block.content.lines() {
            let wrapped = wrap_line_visual(
                Line::from(line.to_string()),
                content_width.saturating_sub(2),
            );
            for wrapped_line in wrapped {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(wrapped_line.spans.iter().map(|span| {
                    Span::styled(span.content.to_string(), span.style.fg(Color::White))
                }));
                lines.push(RenderedChatLine {
                    display: Line::from(spans),
                    copy_projection: Some(CopyProjection {
                        text: format!("  {}", line_plain_text(&wrapped_line)),
                        display_start_col: 0,
                    }),
                });
            }
        }

        lines
    }

    pub(crate) fn tool_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let tool_name = block.tool_name.as_deref().unwrap_or("unknown");
        let mut lines = vec![];

        let arg_summary = if let Some(input) = &block.tool_input {
            Self::tool_arg_summary(tool_name, input)
        } else {
            String::new()
        };

        let (icon_str, icon_color) = match block.tool_status {
            Some(ToolCallStatus::Queued) => ("◌", Color::Blue),
            Some(ToolCallStatus::Running) => ("⏳", Color::Yellow),
            Some(ToolCallStatus::PendingApproval) => ("! ", Color::LightYellow),
            Some(ToolCallStatus::Completed { exit_ok: true }) => ("●", Color::Green),
            Some(ToolCallStatus::Completed { exit_ok: false }) => ("●", Color::Red),
            None => ("●", Color::Gray),
        };

        let header_text = if arg_summary.is_empty() {
            format!("{} {}", icon_str, tool_name)
        } else {
            format!("{} {}({})", icon_str, tool_name, arg_summary)
        };

        let header_style = Style::default().fg(icon_color).add_modifier(Modifier::BOLD);
        lines.push(RenderedChatLine {
            display: Line::from(vec![Span::styled(header_text.clone(), header_style)]),
            copy_projection: Some(CopyProjection {
                text: header_text,
                display_start_col: 0,
            }),
        });
        push_block_header_gap(&mut lines);

        let render_data = Self::build_tool_render_data(block, tool_name);
        let (summary_text, summary_color) = match block.tool_status {
            Some(ToolCallStatus::Queued) => (render_data.summary, Color::Blue),
            Some(ToolCallStatus::Running) => (render_data.summary, Color::Yellow),
            Some(ToolCallStatus::PendingApproval) => (render_data.summary, Color::LightYellow),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                (render_data.summary, Color::Green)
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => (render_data.summary, Color::Red),
            None => (render_data.summary, Color::Gray),
        };

        let summary_style = Style::default().fg(summary_color);
        lines.push(RenderedChatLine {
            display: Line::from(Span::styled(summary_text.clone(), summary_style)),
            copy_projection: Some(CopyProjection {
                text: summary_text,
                display_start_col: 0,
            }),
        });

        let detail_style = Style::default().fg(Color::Gray);
        let output_style = Style::default().fg(Color::White);
        let mut detail_lines = render_data.detail_lines;
        detail_lines.extend(block.tool_progress.iter().map(|line| format!("  {}", line)));
        let preview_lines = render_data.preview_lines;
        let output_lines = render_data.output_lines;
        let has_output = !output_lines.is_empty();
        let has_extra_details = !detail_lines.is_empty() || !preview_lines.is_empty() || has_output;

        if !block.expanded {
            for detail_line in detail_lines.iter() {
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(detail_line.clone(), detail_style)),
                    copy_projection: Some(CopyProjection {
                        text: detail_line.clone(),
                        display_start_col: 0,
                    }),
                });
            }

            for preview_line in preview_lines.iter().take(3) {
                let line = Line::from(preview_line.to_string());
                for wrapped in wrap_line_visual(line, content_width.saturating_sub(2)) {
                    let mut spans = vec![Span::raw("  ")];
                    for span in &wrapped.spans {
                        spans.push(Span::styled(
                            span.content.to_string(),
                            span.style.patch(output_style),
                        ));
                    }
                    lines.push(RenderedChatLine {
                        display: Line::from(spans),
                        copy_projection: Some(CopyProjection {
                            text: format!("  {}", line_plain_text(&wrapped)),
                            display_start_col: 0,
                        }),
                    });
                }
            }

            if has_extra_details {
                let expand_hint = "  (ctrl+o to expand)";
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(
                        expand_hint,
                        detail_style.add_modifier(Modifier::DIM),
                    )),
                    copy_projection: Some(CopyProjection {
                        text: expand_hint.to_string(),
                        display_start_col: 0,
                    }),
                });
            }
        } else {
            for detail_line in detail_lines.iter() {
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(detail_line.clone(), detail_style)),
                    copy_projection: Some(CopyProjection {
                        text: detail_line.clone(),
                        display_start_col: 0,
                    }),
                });
            }

            if has_output {
                let separator = "  ─────────────────";
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(separator, detail_style)),
                    copy_projection: Some(CopyProjection {
                        text: separator.to_string(),
                        display_start_col: 0,
                    }),
                });
            }

            for output in output_lines.iter() {
                let line = Line::from(output.to_string());
                for wrapped in wrap_line_visual(line, content_width.saturating_sub(2)) {
                    let mut spans = vec![Span::raw("  ")];
                    for span in &wrapped.spans {
                        spans.push(Span::styled(
                            span.content.to_string(),
                            span.style.patch(output_style),
                        ));
                    }
                    lines.push(RenderedChatLine {
                        display: Line::from(spans),
                        copy_projection: Some(CopyProjection {
                            text: format!("  {}", line_plain_text(&wrapped)),
                            display_start_col: 0,
                        }),
                    });
                }
            }

            if has_extra_details {
                let collapse_hint = "  (ctrl+o to collapse)";
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(
                        collapse_hint,
                        detail_style.add_modifier(Modifier::DIM),
                    )),
                    copy_projection: Some(CopyProjection {
                        text: collapse_hint.to_string(),
                        display_start_col: 0,
                    }),
                });
            }
        }

        lines
    }

    pub(crate) fn build_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData {
        match tool_name {
            "memory_search" => Self::build_memory_search_render_data(block),
            "exec_command" => Self::build_exec_command_render_data(block),
            "read_file" | "write_file" => Self::build_read_file_render_data(block),
            _ => Self::build_fallback_tool_render_data(block, tool_name),
        }
    }

    fn parse_tool_output_json(content: &str) -> Option<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(content).ok()
    }

    fn compact_preview_text(text: &str, max_chars: usize) -> String {
        let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut chars = compact.chars();
        let preview = chars.by_ref().take(max_chars).collect::<String>();
        if chars.next().is_some() {
            format!("{preview}...")
        } else {
            preview
        }
    }

    fn tool_output_preview_line(
        value: Option<&serde_json::Value>,
        fallback: &str,
    ) -> Option<String> {
        let preferred = value.and_then(|json| {
            json.get("result")
                .and_then(|value| value.as_str())
                .or_else(|| json.get("content").and_then(|value| value.as_str()))
                .or_else(|| json.get("error").and_then(|value| value.as_str()))
        });
        let text = preferred.unwrap_or(fallback);
        let compact = Self::compact_preview_text(text, TOOL_PREVIEW_CHAR_LIMIT);
        (!compact.is_empty()).then_some(format!("preview: {}", compact))
    }

    fn tool_output_lines(content: &str) -> Vec<String> {
        if let Some(value) = Self::parse_tool_output_json(content) {
            serde_json::to_string_pretty(&value)
                .unwrap_or_else(|_| content.to_string())
                .lines()
                .map(|line| line.to_string())
                .collect()
        } else {
            content.lines().map(|line| line.to_string()).collect()
        }
    }

    fn build_memory_search_render_data(block: &ChatBlock) -> ToolRenderData {
        let query = block
            .tool_input
            .as_ref()
            .and_then(|input| input.get("query"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let parsed = serde_json::from_str::<serde_json::Value>(&block.content).ok();
        let results = parsed
            .as_ref()
            .and_then(|value| value.get("results"))
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let hit_count = results.len();
        let timing = Self::timing_suffix(block.timing_ms);
        let summary = match block.tool_status {
            Some(ToolCallStatus::Queued) => "└ Queued…".to_string(),
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::PendingApproval) => "└ Approval needed".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                format!("└ Done ({} hits{})", hit_count, timing)
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => {
                format!("└ Error ({} hits{})", hit_count, timing)
            }
            None => "└ Unknown".to_string(),
        };

        let mut detail_lines = vec![];
        if !query.is_empty() {
            detail_lines.push(format!("  query: {}", query));
        }

        let mut preview_lines = vec![];
        if results.is_empty() && matches!(block.tool_status, Some(ToolCallStatus::Completed { .. }))
        {
            preview_lines.push("0 hits".to_string());
        } else {
            for result in results.iter().take(2) {
                if let Some(path) = result.get("path").and_then(|value| value.as_str()) {
                    preview_lines.push(format!("hit: {}", path));
                }
            }
        }

        let mut output_lines = vec![];
        for (index, result) in results.iter().enumerate() {
            if index > 0 {
                output_lines.push(String::new());
            }
            if let Some(path) = result.get("path").and_then(|value| value.as_str()) {
                output_lines.push(format!("hit: {}", path));
            }
            if let Some(score) = result.get("score").and_then(|value| value.as_f64()) {
                output_lines.push(format!("score: {:.2}", score));
            }
            if let Some(snippet) = result.get("snippet").and_then(|value| value.as_str()) {
                output_lines.extend(
                    snippet
                        .lines()
                        .take(3)
                        .map(|line| format!("snippet: {}", line)),
                );
            }
        }

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn build_exec_command_render_data(block: &ChatBlock) -> ToolRenderData {
        let input = block.tool_input.as_ref();
        let dir = input.and_then(|value| {
            value
                .get("workdir")
                .or_else(|| value.get("cwd"))
                .and_then(|value| value.as_str())
        });
        let parsed = serde_json::from_str::<serde_json::Value>(&block.content).ok();
        let exit_code = parsed
            .as_ref()
            .and_then(|value| value.get("exit_code"))
            .and_then(|value| value.as_i64());
        let output = parsed
            .as_ref()
            .and_then(|value| value.get("output"))
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| block.content.as_str());
        let status = parsed
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str());
        let timing = Self::timing_suffix(block.timing_ms);

        let summary = match block.tool_status {
            Some(ToolCallStatus::Queued) => "└ Queued…".to_string(),
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::PendingApproval) => "└ Approval needed".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                if let Some(exit) = exit_code {
                    format!("└ Done (exit:{}{})", exit, timing)
                } else {
                    format!("└ Done{}", timing)
                }
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => {
                if let Some(exit) = exit_code {
                    format!("└ Error (exit:{}{})", exit, timing)
                } else {
                    format!("└ Error{}", timing)
                }
            }
            None => "└ Unknown".to_string(),
        };

        let mut detail_lines = vec![];
        if let Some(dir) = dir {
            detail_lines.push(format!("  dir:  {}", dir));
        }
        if let Some(status) = status {
            let normalized = match status {
                "exited" => "success",
                other => other,
            };
            detail_lines.push(format!("  status: {}", normalized));
        }

        let mut preview_lines = vec![];
        if let Some(first_line) = output.lines().find(|line| !line.trim().is_empty()) {
            preview_lines.push(format!("stdout: {}", first_line));
        }

        let output_lines = output
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn build_read_file_render_data(block: &ChatBlock) -> ToolRenderData {
        let path = block
            .tool_input
            .as_ref()
            .and_then(|input| input.get("file_path").or_else(|| input.get("path")))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let parsed = Self::parse_tool_output_json(&block.content);
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
            .unwrap_or_else(|| block.content.as_str());
        let line_count = primary_text.lines().count();
        let timing = Self::timing_suffix(block.timing_ms);
        let summary = match block.tool_status {
            Some(ToolCallStatus::Queued) => "└ Queued…".to_string(),
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::PendingApproval) => "└ Approval needed".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                format!("└ Done ({} lines{})", line_count, timing)
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => {
                format!("└ Error ({} lines{})", line_count, timing)
            }
            None => "└ Unknown".to_string(),
        };

        let mut detail_lines = if path.is_empty() {
            vec![]
        } else {
            vec![format!("  path:  {}", path)]
        };
        if let Some(truncated) = parsed
            .as_ref()
            .and_then(|value| value.get("truncated"))
            .and_then(|value| value.as_bool())
        {
            detail_lines.push(format!("  truncated: {}", truncated));
        }
        let preview_lines = Self::tool_output_preview_line(parsed.as_ref(), primary_text)
            .into_iter()
            .collect::<Vec<_>>();
        let output_lines = Self::tool_output_lines(&block.content);

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn build_fallback_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData {
        let parsed = Self::parse_tool_output_json(&block.content);
        let detail_lines = block
            .tool_input
            .as_ref()
            .map(|input| Self::fallback_tool_detail_lines(tool_name, input))
            .unwrap_or_default();
        let preview_lines = Self::tool_output_preview_line(parsed.as_ref(), &block.content)
            .into_iter()
            .collect::<Vec<_>>();
        let output_lines = Self::tool_output_lines(&block.content);
        let timing = Self::timing_suffix(block.timing_ms);
        let summary = match block.tool_status {
            Some(ToolCallStatus::Queued) => "└ Queued…".to_string(),
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::PendingApproval) => "└ Approval needed".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => format!("└ Done{}", timing),
            Some(ToolCallStatus::Completed { exit_ok: false }) => format!("└ Error{}", timing),
            None => "└ Unknown".to_string(),
        };

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn fallback_tool_detail_lines(tool_name: &str, input: &serde_json::Value) -> Vec<String> {
        match tool_name {
            "web_search" => input
                .get("query")
                .and_then(|v| v.as_str())
                .map(|query| vec![format!("  query: {}", query)])
                .unwrap_or_default(),
            "http_request" => input
                .get("url")
                .and_then(|v| v.as_str())
                .map(|url| {
                    let method = input
                        .get("method")
                        .and_then(|v| v.as_str())
                        .unwrap_or("GET");
                    vec![format!("  {} {}", method, url)]
                })
                .unwrap_or_default(),
            _ => input
                .as_object()
                .and_then(|m| {
                    m.iter().find_map(|(key, value)| {
                        value.as_str().map(|value| format!("  {}: {}", key, value))
                    })
                })
                .map(|line| vec![line])
                .unwrap_or_default(),
        }
    }

    fn timing_suffix(timing_ms: Option<u64>) -> String {
        timing_ms
            .map(|ms| format!(" · {:.1}s", ms as f64 / 1000.0))
            .unwrap_or_default()
    }

    fn join_tool_summary_parts(parts: Vec<String>) -> String {
        parts
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn tool_arg_summary(tool_name: &str, input: &serde_json::Value) -> String {
        let summary = match tool_name {
            "exec_command" => input
                .get("cmd")
                .or_else(|| input.get("command"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "read_file" => input
                .get("file_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "write_file" => input
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "list_dir" => input
                .get("dir_path")
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "grep_files" => {
                let mut parts = vec![];
                if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                    parts.push(format!("{:?}", pattern));
                }
                if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                    parts.push(format!("path={}", path));
                }
                if let Some(include) = input.get("include").and_then(|v| v.as_str()) {
                    parts.push(format!("include={}", include));
                }
                Self::join_tool_summary_parts(parts)
            }
            "memory_search" => input
                .get("query")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "web_search" => input
                .get("query")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "http_request" => input
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            _ => input
                .as_object()
                .and_then(|m| {
                    let parts = m
                        .iter()
                        .filter_map(|(key, value)| {
                            value.as_str().map(|value| format!("{}={}", key, value))
                        })
                        .take(2)
                        .collect::<Vec<_>>();
                    (!parts.is_empty()).then(|| Self::join_tool_summary_parts(parts))
                })
                .unwrap_or_default(),
        };
        if summary.len() > 70 {
            format!("{}...", &summary[..67])
        } else {
            summary
        }
    }
}
