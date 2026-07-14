use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::render::helpers;
use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::ratatui::to_ratatui_line;
use crate::render_runtime::transcript::block::TranscriptBlockKind;
use crate::render_runtime::transcript::line::TranscriptLineId;
use crate::types::{ChatRenderCacheKey, RenderedChatLine};
use crate::utils::{
    build_welcome_card, line_plain_text, text_display_width, wrap_line_visual, APP_VERSION,
};
use crate::{TuiApp, SPINNER_FRAMES, WELCOME_TIP_LINE};

const CHAT_VISIBLE_OVERSCAN: usize = 2;
const TRANSCRIPT_WRAP_RESERVE: usize = 2;

pub(crate) fn transcript_content_width(chat_width: usize) -> usize {
    chat_width.saturating_sub(TRANSCRIPT_WRAP_RESERVE).max(1)
}

struct TranscriptRenderBlock {
    kind: TranscriptBlockKind,
    lines: Vec<(Option<TranscriptLineId>, StyledLine)>,
    round_duration_ms: Option<u64>,
}

/// Window of chat lines passed to Ratatui for the current viewport.
pub(crate) struct VisibleChatWindow {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) local_scroll: u16,
}

impl TuiApp {
    pub(crate) fn rebuild_chat_render_state_if_rect(&mut self) {
        if let Some(rect) = self.chat_content_rect {
            self.rebuild_chat_render_state(rect);
        }
    }

    pub(crate) fn rebuild_chat_render_state(&mut self, chat_content_rect: Rect) {
        self.chat_content_rect = Some(chat_content_rect);
        self.chat_viewport_height = chat_content_rect.height as usize;
        self.rebuild_transcript_render_state(chat_content_rect);
    }

    pub(crate) fn projected_live_chat_lines(&self, chat_width: u16) -> Vec<RenderedChatLine> {
        let content_width = transcript_content_width(chat_width as usize);
        let mut rendered = Vec::new();
        for block in self.render_runtime.live_projection_blocks() {
            if !rendered.is_empty() {
                rendered.push(helpers::blank_chat_line());
            }
            let lines = block
                .lines
                .into_iter()
                .map(|line| (None, line))
                .collect::<Vec<_>>();
            rendered.extend(render_live_transcript_block_lines(
                block.kind,
                &lines,
                content_width,
                self.spinner_index,
            ));
        }
        if self.is_agent_working() {
            if rendered
                .last()
                .is_some_and(|line| !line_plain_text(&line.display).is_empty())
            {
                rendered.push(helpers::blank_chat_line());
            }
            rendered.push(self.thinking_spinner_line(self.spinner_index));
        }
        rendered
    }

    pub(crate) fn projected_live_chat_line_count(&self, chat_width: u16) -> usize {
        self.projected_live_chat_lines(chat_width).len()
            + usize::from(self.inline_viewport_needs_leading_history_gap())
    }

    fn rebuild_transcript_render_state(&mut self, chat_content_rect: Rect) {
        let cache_key = self.transcript_render_cache_key(chat_content_rect);
        if self.rendered_chat_cache_key == Some(cache_key) {
            return;
        }

        self.rendered_chat_lines.clear();
        let content_width = transcript_content_width(chat_content_rect.width as usize);
        let finalized_blocks = self
            .render_runtime
            .store()
            .blocks()
            .iter()
            .filter(|block| !block.terminal_synced)
            .map(|block| TranscriptRenderBlock {
                kind: block.kind,
                lines: block
                    .lines
                    .iter()
                    .map(|line| (Some(line.id), line.content.clone()))
                    .collect::<Vec<_>>(),
                round_duration_ms: block.round_duration_ms,
            })
            .collect::<Vec<_>>();
        if self.inline_viewport_needs_leading_history_gap() {
            self.rendered_chat_lines.push(helpers::blank_chat_line());
        }
        let mut block_index = 0usize;
        while block_index < finalized_blocks.len() {
            let round_start = block_index;
            let round_end_exclusive = ((round_start + 1)..finalized_blocks.len())
                .find(|&index| is_transcript_segment_start(finalized_blocks[index].kind))
                .unwrap_or(finalized_blocks.len());

            let round_blocks = &finalized_blocks[round_start..round_end_exclusive];
            for (index, block) in round_blocks.iter().enumerate() {
                self.rendered_chat_lines
                    .extend(render_transcript_block_lines(
                        block.kind,
                        &block.lines,
                        content_width,
                    ));
                if index + 1 < round_blocks.len() {
                    self.rendered_chat_lines.push(helpers::blank_chat_line());
                }
            }

            if self.transcript_round_is_completed(
                &finalized_blocks,
                round_start,
                round_end_exclusive,
            ) {
                self.rendered_chat_lines.push(helpers::blank_chat_line());
                let duration_ms = self.transcript_round_duration_ms(
                    &finalized_blocks,
                    round_start,
                    round_end_exclusive,
                );
                let footer_line = self.round_footer_line_with_duration(duration_ms);
                self.rendered_chat_lines.push(footer_line);
            }

            if round_end_exclusive < finalized_blocks.len() {
                self.rendered_chat_lines.push(helpers::blank_chat_line());
            }
            block_index = round_end_exclusive;
        }

        let has_live_blocks = !self.render_runtime.live_projection_blocks().is_empty();
        if self.rendered_chat_lines.is_empty()
            && self.render_runtime.store().blocks().is_empty()
            && !has_live_blocks
        {
            self.push_empty_transcript_state_lines();
        }
        let live_lines = self.projected_live_chat_lines(chat_content_rect.width);
        if !live_lines.is_empty() {
            if self
                .rendered_chat_lines
                .last()
                .is_some_and(|line| !line_plain_text(&line.display).is_empty())
            {
                self.rendered_chat_lines.push(helpers::blank_chat_line());
            }
            self.rendered_chat_lines.extend(live_lines);
        }
        self.rendered_chat_cache_key = Some(cache_key);
    }

    fn push_empty_transcript_state_lines(&mut self) {
        let welcome = build_welcome_card(
            APP_VERSION,
            &self.current_model,
            self.current_project_root.as_deref(),
        );
        for line in welcome.lines() {
            self.rendered_chat_lines.push(RenderedChatLine {
                display: Line::from(line.to_string()),
            });
        }
        self.rendered_chat_lines.push(RenderedChatLine {
            display: Line::from(WELCOME_TIP_LINE),
        });
    }

    fn inline_viewport_needs_leading_history_gap(&self) -> bool {
        let blocks = self.render_runtime.store().blocks();
        let Some(first_visible_index) = blocks.iter().position(|block| !block.terminal_synced)
        else {
            return !blocks.is_empty()
                && (!self.render_runtime.live_projection_lines().is_empty()
                    || self.is_agent_working());
        };
        blocks[..first_visible_index]
            .iter()
            .any(|block| block.terminal_synced)
    }

    fn transcript_round_is_completed(
        &self,
        blocks: &[TranscriptRenderBlock],
        round_start: usize,
        round_end_exclusive: usize,
    ) -> bool {
        if round_start >= blocks.len() || blocks[round_start].kind != TranscriptBlockKind::UserInput
        {
            return false;
        }
        if round_end_exclusive < blocks.len() {
            return true;
        }
        if self.in_flight || self.turn_metrics_live {
            return false;
        }
        blocks[round_start..round_end_exclusive]
            .iter()
            .any(|block| {
                matches!(
                    block.kind,
                    TranscriptBlockKind::Commentary | TranscriptBlockKind::FinalAnswer
                )
            })
    }

    fn transcript_round_duration_ms(
        &self,
        blocks: &[TranscriptRenderBlock],
        round_start: usize,
        round_end_exclusive: usize,
    ) -> Option<u64> {
        if let Some(duration_ms) = blocks[round_start].round_duration_ms {
            return Some(duration_ms);
        }
        if round_end_exclusive == blocks.len() && !self.in_flight && !self.turn_metrics_live {
            return self
                .turn_metrics
                .as_ref()
                .and_then(|metrics| metrics.duration_ms);
        }
        None
    }

    fn transcript_render_cache_key(&self, chat_content_rect: Rect) -> ChatRenderCacheKey {
        let mut hasher = DefaultHasher::new();
        "transcript".hash(&mut hasher);
        self.current_model.hash(&mut hasher);
        self.current_project_root.hash(&mut hasher);
        format!("{:?}", self.turn_metrics).hash(&mut hasher);
        format!("{:?}", self.render_runtime.store().blocks()).hash(&mut hasher);
        format!("{:?}", self.render_runtime.interactive_projection_lines()).hash(&mut hasher);
        ChatRenderCacheKey {
            content_width: chat_content_rect.width,
            viewport_height: chat_content_rect.height,
            block_count: self.render_runtime.interactive_block_count(),
            blocks_fingerprint: hasher.finish(),
            in_flight: self.in_flight,
            spinner_index: self.is_agent_working().then_some(self.spinner_index),
        }
    }

    fn round_footer_line_with_duration(&self, duration_ms: Option<u64>) -> RenderedChatLine {
        let duration_text = duration_ms.map_or_else(
            || "✻ Brewed for a moment".to_string(),
            Self::format_round_duration,
        );

        RenderedChatLine {
            display: Line::from(Span::styled(
                duration_text.clone(),
                Style::default().fg(Color::White),
            )),
        }
    }

    pub(crate) fn format_round_duration(duration_ms: u64) -> String {
        let total_seconds = duration_ms / 1000;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        if minutes > 0 {
            format!("✻ Brewed for {minutes}m {seconds}s")
        } else if total_seconds > 0 {
            format!("✻ Brewed for {total_seconds}s")
        } else {
            format!("✻ Brewed for {:.1}s", duration_ms as f64 / 1000.0)
        }
    }

    pub(crate) fn visible_chat_window(&self) -> VisibleChatWindow {
        if self.chat_viewport_height == 0 || self.rendered_chat_lines.is_empty() {
            return VisibleChatWindow {
                lines: Vec::new(),
                local_scroll: 0,
            };
        }
        let max_scroll = self
            .rendered_chat_lines
            .len()
            .saturating_sub(self.chat_viewport_height);
        self.visible_chat_window_with_overscan(
            max_scroll,
            self.chat_viewport_height,
            CHAT_VISIBLE_OVERSCAN,
        )
    }

    pub(crate) fn visible_chat_window_with_overscan(
        &self,
        scroll_offset: usize,
        viewport_height: usize,
        overscan: usize,
    ) -> VisibleChatWindow {
        if viewport_height == 0 || self.rendered_chat_lines.is_empty() {
            return VisibleChatWindow {
                lines: Vec::new(),
                local_scroll: 0,
            };
        }

        let max_scroll = self
            .rendered_chat_lines
            .len()
            .saturating_sub(viewport_height);
        let bounded_scroll = scroll_offset.min(max_scroll);
        let start = bounded_scroll.saturating_sub(overscan);
        let end = bounded_scroll
            .saturating_add(viewport_height)
            .saturating_add(overscan)
            .min(self.rendered_chat_lines.len());
        let local_scroll = bounded_scroll.saturating_sub(start).min(u16::MAX as usize) as u16;
        let lines = self.rendered_chat_lines[start..end]
            .iter()
            .map(|line| line.display.clone())
            .collect();

        VisibleChatWindow {
            lines,
            local_scroll,
        }
    }
}

pub(crate) fn render_transcript_block_lines(
    kind: TranscriptBlockKind,
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
) -> Vec<RenderedChatLine> {
    match kind {
        TranscriptBlockKind::Thinking => {
            let content = transcript_projection_plain_text(lines);
            thinking_preview_lines(&content, content_width)
        }
        TranscriptBlockKind::SessionHeader => plain_styled_transcript_lines(lines, content_width),
        TranscriptBlockKind::CommandInput => command_transcript_lines(lines, content_width),
        TranscriptBlockKind::Tool => tool_transcript_lines(lines, content_width),
        TranscriptBlockKind::UserInput => user_transcript_lines(lines, content_width),
        TranscriptBlockKind::Commentary => commentary_transcript_lines(lines, content_width),
        TranscriptBlockKind::FinalAnswer => final_answer_transcript_lines(lines, content_width),
        TranscriptBlockKind::SystemNotice => plain_styled_transcript_lines(lines, content_width),
    }
}

fn is_transcript_segment_start(kind: TranscriptBlockKind) -> bool {
    matches!(
        kind,
        TranscriptBlockKind::UserInput | TranscriptBlockKind::CommandInput
    )
}

fn render_live_transcript_block_lines(
    kind: TranscriptBlockKind,
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
    spinner_index: usize,
) -> Vec<RenderedChatLine> {
    if kind == TranscriptBlockKind::Tool {
        return live_tool_transcript_lines(lines, content_width, spinner_index);
    }
    render_transcript_block_lines(kind, lines, content_width)
}

fn transcript_projection_plain_text(lines: &[(Option<TranscriptLineId>, StyledLine)]) -> String {
    lines
        .iter()
        .map(|(_, line)| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n")
}

fn plain_styled_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
) -> Vec<RenderedChatLine> {
    let mut rendered = Vec::new();
    for (_, line) in lines {
        let plain_text = line.plain_text();
        let plain_lines = plain_text.lines().collect::<Vec<_>>();
        if plain_lines.len() <= 1 {
            let wrapped = wrap_line_visual(to_ratatui_line(line), content_width);
            for display in wrapped {
                rendered.push(RenderedChatLine { display });
            }
            continue;
        }
        for raw_line in plain_lines {
            let wrapped = wrap_line_visual(Line::from(raw_line.to_string()), content_width);
            for display in wrapped {
                rendered.push(RenderedChatLine { display });
            }
        }
    }
    rendered
}

fn user_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
) -> Vec<RenderedChatLine> {
    let content = transcript_projection_plain_text(lines);
    let body_style = if content.trim_start().starts_with('/') {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    bullet_transcript_lines(
        lines,
        content_width,
        Style::default().fg(Color::Gray),
        body_style,
    )
}

fn command_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
) -> Vec<RenderedChatLine> {
    let body_style = Style::default()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::BOLD);
    let mut rendered = Vec::new();
    for (_, line) in lines {
        let wrapped = wrap_line_visual(
            Line::from(Span::styled(line.plain_text(), body_style)),
            content_width,
        );
        for display in wrapped {
            rendered.push(RenderedChatLine { display });
        }
    }
    rendered
}

fn commentary_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
) -> Vec<RenderedChatLine> {
    bullet_transcript_lines(
        lines,
        content_width,
        Style::default().fg(Color::Gray),
        Style::default().fg(Color::White),
    )
}

fn final_answer_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
) -> Vec<RenderedChatLine> {
    bullet_transcript_lines(
        lines,
        content_width,
        Style::default().fg(Color::Gray),
        Style::default().fg(Color::White),
    )
}

fn bullet_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
    bullet_style: Style,
    body_style: Style,
) -> Vec<RenderedChatLine> {
    let mut rendered = Vec::new();
    let mut first_content_line = true;
    for (_, line) in lines {
        let plain_text = line.plain_text();
        for raw_line in plain_text.lines() {
            let prefix = if first_content_line { "● " } else { "  " };
            let prefix_style = if first_content_line {
                bullet_style
            } else {
                Style::default()
            };
            first_content_line = false;
            let prefix_width = text_display_width(prefix);
            let wrapped = wrap_line_visual(
                Line::from(Span::styled(raw_line.to_string(), body_style)),
                content_width.saturating_sub(prefix_width),
            );
            for (index, wrapped_line) in wrapped.into_iter().enumerate() {
                let actual_prefix = if index == 0 { prefix } else { "  " };
                let display = Line::from(
                    std::iter::once(Span::styled(actual_prefix.to_string(), prefix_style))
                        .chain(wrapped_line.spans)
                        .collect::<Vec<_>>(),
                );
                rendered.push(RenderedChatLine { display });
            }
        }
    }

    if rendered.is_empty() {
        rendered.push(RenderedChatLine {
            display: Line::from(vec![
                Span::styled("● ", bullet_style),
                Span::styled(String::new(), body_style),
            ]),
        });
    }
    rendered
}

fn tool_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
) -> Vec<RenderedChatLine> {
    let mut rendered = Vec::new();
    let has_detail = lines
        .iter()
        .any(|(_, line)| line.plain_text().starts_with("    "));
    for (index, (_, line)) in lines.iter().enumerate() {
        let text = line.plain_text();
        if let Some(title) = text.strip_prefix("● ") {
            rendered.push(RenderedChatLine {
                display: Line::from(vec![
                    Span::styled(
                        "●",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        title.to_string(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]),
            });
        } else if let Some(evidence) = text.strip_prefix("  ⎿ ") {
            let evidence = if index == 1 && has_detail {
                format!("{evidence} (ctrl+o to expand)")
            } else {
                evidence.to_string()
            };
            rendered.push(RenderedChatLine {
                display: Line::from(vec![
                    Span::styled("  ⎿ ", Style::default().fg(Color::Gray)),
                    Span::styled(evidence, Style::default().fg(Color::Gray)),
                ]),
            });
        } else if let Some(output) = text.strip_prefix("  │ ") {
            push_visible_tool_output(&mut rendered, output, content_width);
        } else if let Some(detail) = text.strip_prefix("    ") {
            if !has_detail {
                push_indented_tool_detail(&mut rendered, detail, content_width);
            }
        } else {
            push_indented_tool_detail(&mut rendered, &text, content_width);
        }
    }
    rendered
}

fn live_tool_transcript_lines(
    lines: &[(Option<TranscriptLineId>, StyledLine)],
    content_width: usize,
    spinner_index: usize,
) -> Vec<RenderedChatLine> {
    let mut rendered = Vec::new();
    let marker = SPINNER_FRAMES[spinner_index % SPINNER_FRAMES.len()];
    for (_, line) in lines {
        let text = line.plain_text();
        if let Some(title) = text
            .strip_prefix("● ")
            .and_then(|title| title.strip_suffix(" running"))
        {
            rendered.push(RenderedChatLine {
                display: Line::from(vec![
                    Span::styled(
                        marker.to_string(),
                        Style::default()
                            .fg(helpers::spinning_color(spinner_index))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        title.to_string(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]),
            });
        } else if let Some(evidence) = text.strip_prefix("  ⎿ ") {
            rendered.push(RenderedChatLine {
                display: Line::from(vec![
                    Span::styled("  ⎿ ", Style::default().fg(Color::Gray)),
                    Span::styled(evidence.to_string(), Style::default().fg(Color::Gray)),
                ]),
            });
        } else if let Some(output) = text.strip_prefix("  │ ") {
            push_visible_tool_output(&mut rendered, output, content_width);
        } else if let Some(detail) = text.strip_prefix("    ") {
            push_indented_tool_detail(&mut rendered, detail, content_width);
        } else {
            push_indented_tool_detail(&mut rendered, &text, content_width);
        }
    }
    rendered
}

fn push_visible_tool_output(lines: &mut Vec<RenderedChatLine>, text: &str, content_width: usize) {
    let style = if text.starts_with('+') && !text.starts_with("+++") {
        Style::default()
            .fg(Color::LightGreen)
            .bg(Color::Rgb(18, 58, 36))
    } else if text.starts_with('-') && !text.starts_with("---") {
        Style::default()
            .fg(Color::LightRed)
            .bg(Color::Rgb(78, 32, 28))
    } else {
        Style::default().fg(Color::Gray)
    };
    let wrapped = wrap_line_visual(
        Line::from(Span::styled(text.to_string(), style)),
        content_width,
    );
    for line in wrapped {
        let display = Line::from(
            std::iter::once(Span::raw("    "))
                .chain(line.spans)
                .collect::<Vec<_>>(),
        );
        lines.push(RenderedChatLine { display });
    }
}

fn push_indented_tool_detail(lines: &mut Vec<RenderedChatLine>, text: &str, content_width: usize) {
    let wrapped = wrap_line_visual(
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(Color::Gray),
        )),
        content_width,
    );
    for line in wrapped {
        let display = Line::from(
            std::iter::once(Span::raw("    "))
                .chain(line.spans)
                .collect::<Vec<_>>(),
        );
        lines.push(RenderedChatLine { display });
    }
}

fn thinking_preview_lines(content: &str, content_width: usize) -> Vec<RenderedChatLine> {
    if content.is_empty() {
        return Vec::new();
    }
    let body_style = Style::default()
        .fg(Color::Gray)
        .add_modifier(Modifier::ITALIC | Modifier::DIM);
    let preview = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    let text = if preview.is_empty() {
        "Thinking (ctrl+o to expand)".to_string()
    } else {
        format!("Thinking: {preview} (ctrl+o to expand)")
    };
    prefix_transcript_lines(
        &text,
        content_width,
        "∴ ",
        Style::default().fg(Color::Gray),
        body_style,
    )
}

fn prefix_transcript_lines(
    content: &str,
    content_width: usize,
    prefix: &str,
    prefix_style: Style,
    body_style: Style,
) -> Vec<RenderedChatLine> {
    prefixed_transcript_lines(content, content_width, prefix, prefix_style, body_style)
}

fn prefixed_transcript_lines(
    content: &str,
    content_width: usize,
    first_prefix: &str,
    first_prefix_style: Style,
    body_style: Style,
) -> Vec<RenderedChatLine> {
    let mut lines = Vec::new();
    let mut first_content_line = true;

    for raw_line in content.lines() {
        let prefix = if first_content_line {
            first_prefix
        } else {
            "  "
        };
        let prefix_style = if first_content_line {
            first_prefix_style
        } else {
            Style::default()
        };
        first_content_line = false;
        let prefix_width = text_display_width(prefix);
        let wrapped_lines = wrap_line_visual(
            Line::from(Span::styled(raw_line.to_string(), body_style)),
            content_width.saturating_sub(prefix_width),
        );
        for (index, wrapped) in wrapped_lines.into_iter().enumerate() {
            let actual_prefix = if index == 0 { prefix } else { "  " };
            let display = Line::from(
                std::iter::once(Span::styled(actual_prefix.to_string(), prefix_style))
                    .chain(wrapped.spans)
                    .collect::<Vec<_>>(),
            );
            lines.push(RenderedChatLine { display });
        }
    }

    if lines.is_empty() {
        lines.push(RenderedChatLine {
            display: Line::from(vec![
                Span::styled(first_prefix.to_string(), first_prefix_style),
                Span::styled(String::new(), body_style),
            ]),
        });
    }

    lines
}
