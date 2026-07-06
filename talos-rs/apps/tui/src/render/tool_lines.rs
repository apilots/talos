use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::render::tool;
use crate::types::{ChatBlock, CopyProjection, RenderedChatLine, ToolCallStatus, ToolRenderData};
use crate::utils::{line_plain_text, wrap_line_visual};
use crate::{TuiApp, SPINNER_FRAMES};

impl TuiApp {
    pub(crate) fn tool_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let tool_name = block.tool_name.as_deref().unwrap_or("unknown");
        let render_data = tool::build_tool_render_data(block, tool_name);
        let mut lines = Vec::new();

        lines.push(render_tool_header(
            block.tool_status.as_ref(),
            &render_data.title,
            self.spinner_index,
        ));

        if block.expanded || render_data.force_show_output {
            push_expanded_tool_lines(&mut lines, &render_data, content_width);
        } else {
            push_collapsed_tool_lines(&mut lines, &render_data);
        }

        lines
    }
}

fn render_tool_header(
    status: Option<&ToolCallStatus>,
    title: &str,
    spinner_index: usize,
) -> RenderedChatLine {
    let (marker, color) = status_marker(status, spinner_index);
    let text = format!("{marker} {title}");
    RenderedChatLine {
        display: Line::from(vec![
            Span::styled(
                marker.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                title.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        copy_projection: Some(CopyProjection {
            text,
            display_start_col: 0,
        }),
    }
}

fn push_collapsed_tool_lines(lines: &mut Vec<RenderedChatLine>, render_data: &ToolRenderData) {
    let has_extra = !render_data.detail_lines.is_empty()
        || !render_data.preview_lines.is_empty()
        || !render_data.output_lines.is_empty();
    let mut evidence = render_data.summary.clone();
    if has_extra {
        evidence.push_str(" (ctrl+o to expand)");
    }
    push_evidence_line(lines, &evidence, EvidenceKind::Summary);
}

fn push_expanded_tool_lines(
    lines: &mut Vec<RenderedChatLine>,
    render_data: &ToolRenderData,
    content_width: usize,
) {
    push_evidence_line(lines, &render_data.summary, EvidenceKind::Summary);

    for detail in &render_data.detail_lines {
        push_continuation_line(lines, detail, EvidenceKind::Detail, content_width);
    }

    if !render_data.output_lines.is_empty() {
        for output in &render_data.output_lines {
            if output.is_empty() {
                lines.push(RenderedChatLine {
                    display: Line::from(""),
                    copy_projection: Some(CopyProjection {
                        text: String::new(),
                        display_start_col: 0,
                    }),
                });
                continue;
            }
            push_continuation_line(lines, output, output_kind(output), content_width);
        }
    } else {
        for preview in &render_data.preview_lines {
            push_continuation_line(lines, preview, EvidenceKind::Detail, content_width);
        }
    }

    if !render_data.force_show_output {
        push_continuation_line(
            lines,
            "(ctrl+o to collapse)",
            EvidenceKind::Hint,
            content_width,
        );
    }
}

fn push_evidence_line(lines: &mut Vec<RenderedChatLine>, text: &str, kind: EvidenceKind) {
    let line_text = format!("  ⎿ {text}");
    lines.push(RenderedChatLine {
        display: Line::from(vec![
            Span::styled("  ⎿ ", Style::default().fg(Color::Gray)),
            Span::styled(text.to_string(), evidence_style(kind)),
        ]),
        copy_projection: Some(CopyProjection {
            text: line_text,
            display_start_col: 0,
        }),
    });
}

fn push_continuation_line(
    lines: &mut Vec<RenderedChatLine>,
    text: &str,
    kind: EvidenceKind,
    content_width: usize,
) {
    let style = evidence_style(kind);
    let wrapped = wrap_line_visual(
        Line::from(Span::styled(text.to_string(), style)),
        content_width,
    );
    for line in wrapped {
        let plain = line_plain_text(&line);
        let display = Line::from(
            std::iter::once(Span::raw("    "))
                .chain(line.spans)
                .collect::<Vec<_>>(),
        );
        lines.push(RenderedChatLine {
            display,
            copy_projection: Some(CopyProjection {
                text: format!("    {plain}"),
                display_start_col: 0,
            }),
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum EvidenceKind {
    Summary,
    Detail,
    Hint,
    Added,
    Removed,
}

fn status_marker(status: Option<&ToolCallStatus>, spinner_index: usize) -> (String, Color) {
    match status {
        Some(ToolCallStatus::Queued) => ("●".to_string(), Color::DarkGray),
        Some(ToolCallStatus::Running) => (
            SPINNER_FRAMES[spinner_index % SPINNER_FRAMES.len()].to_string(),
            crate::render::helpers::spinning_color(spinner_index),
        ),
        Some(ToolCallStatus::PendingDynamicInput) => ("●".to_string(), Color::Yellow),
        Some(ToolCallStatus::Completed { exit_ok: true }) => ("●".to_string(), Color::Green),
        Some(ToolCallStatus::Completed { exit_ok: false }) => ("●".to_string(), Color::Red),
        None => ("●".to_string(), Color::Gray),
    }
}

fn evidence_style(kind: EvidenceKind) -> Style {
    match kind {
        EvidenceKind::Summary => Style::default().fg(Color::Gray),
        EvidenceKind::Detail => Style::default().fg(Color::Gray),
        EvidenceKind::Hint => Style::default().fg(Color::DarkGray),
        EvidenceKind::Added => Style::default()
            .fg(Color::LightGreen)
            .bg(Color::Rgb(18, 58, 36)),
        EvidenceKind::Removed => Style::default()
            .fg(Color::LightRed)
            .bg(Color::Rgb(78, 32, 28)),
    }
}

fn output_kind(text: &str) -> EvidenceKind {
    if text.starts_with('+') && !text.starts_with("+++") {
        EvidenceKind::Added
    } else if text.starts_with('-') && !text.starts_with("---") {
        EvidenceKind::Removed
    } else {
        EvidenceKind::Detail
    }
}
