use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::markdown::render_markdown_lines;
use crate::render::helpers;
use crate::types::{ChatBlock, ChatRole, CopyProjection, RenderedChatLine, RoundFooterButton};
use crate::utils::{
    highlight_line_columns, line_display_width, line_plain_text, text_display_width,
    wrap_line_visual,
};
use crate::{TuiApp, CHAT_BLOCK_BODY_INDENT, COPY_ROUND_BUTTON_TEXT};

impl TuiApp {
    pub(crate) fn rebuild_chat_render_state_if_rect(&mut self) {
        if let Some(rect) = self.chat_content_rect {
            self.rebuild_chat_render_state(rect);
        }
    }

    pub(crate) fn rebuild_chat_render_state(&mut self, chat_content_rect: Rect) {
        self.chat_content_rect = Some(chat_content_rect);
        self.chat_viewport_height = chat_content_rect.height as usize;
        self.rendered_chat_lines.clear();
        self.rendered_round_footer_buttons.clear();

        let content_width = chat_content_rect.width.saturating_sub(2).max(1) as usize;
        let mut block_index = 0usize;
        while block_index < self.chat_blocks.len() {
            let round_start = block_index;
            let round_end_exclusive = ((round_start + 1)..self.chat_blocks.len())
                .find(|&index| self.chat_blocks[index].role == ChatRole::User)
                .unwrap_or(self.chat_blocks.len());

            let round_blocks = &self.chat_blocks[round_start..round_end_exclusive];
            for (index, block) in round_blocks.iter().enumerate() {
                self.rendered_chat_lines
                    .extend(self.chat_card_lines(block, content_width));
                if index + 1 < round_blocks.len() {
                    self.rendered_chat_lines.push(helpers::blank_chat_line());
                }
            }

            if self.round_is_completed(round_start, round_end_exclusive) {
                self.rendered_chat_lines.push(helpers::blank_chat_line());
                let footer_line_index = self.rendered_chat_lines.len();
                let (footer_line, button) = self.round_footer_line(round_start, content_width);
                self.rendered_chat_lines.push(footer_line);
                self.rendered_round_footer_buttons
                    .insert(footer_line_index, button);
            }

            self.rendered_chat_lines.push(helpers::blank_chat_line());
            block_index = round_end_exclusive;
        }

        // Append thinking spinner if agent is working
        if self.is_agent_working() {
            self.rendered_chat_lines
                .push(self.thinking_spinner_line(self.spinner_index));
        }

        if self.rendered_chat_lines.is_empty() {
            self.rendered_chat_lines.push(RenderedChatLine {
                display: Line::from("No messages yet."),
                copy_projection: Some(CopyProjection {
                    text: "No messages yet.".to_string(),
                    display_start_col: 0,
                }),
            });
        }
    }

    pub(crate) fn round_is_completed(
        &self,
        round_start: usize,
        round_end_exclusive: usize,
    ) -> bool {
        if round_start >= self.chat_blocks.len() {
            return false;
        }
        if self.chat_blocks[round_start].role != ChatRole::User {
            return false;
        }
        if round_end_exclusive < self.chat_blocks.len() {
            return true;
        }
        !self.in_flight
            && self.chat_blocks[round_start..round_end_exclusive]
                .iter()
                .any(|block| matches!(block.role, ChatRole::Assistant | ChatRole::System))
    }

    pub(crate) fn round_footer_line(
        &self,
        round_start: usize,
        content_width: usize,
    ) -> (RenderedChatLine, RoundFooterButton) {
        let duration_text = self.chat_blocks[round_start].round_timing_ms.map_or_else(
            || "✻ Brewed for a moment".to_string(),
            Self::format_round_duration,
        );
        let button_text = COPY_ROUND_BUTTON_TEXT.to_string();
        let duration_width = text_display_width(&duration_text);
        let button_width = text_display_width(&button_text);
        let spacing_width = content_width
            .saturating_sub(duration_width.saturating_add(button_width))
            .max(2);
        let spacing = " ".repeat(spacing_width);
        let button_start_col = duration_width + spacing_width;
        let line_text = format!("{duration_text}{spacing}{button_text}");

        (
            RenderedChatLine {
                display: Line::from(vec![
                    Span::styled(duration_text.clone(), Style::default().fg(Color::White)),
                    Span::raw(spacing),
                    Span::styled(
                        button_text.clone(),
                        Style::default()
                            .fg(Color::Rgb(180, 190, 210))
                            .add_modifier(Modifier::DIM),
                    ),
                ]),
                copy_projection: Some(CopyProjection {
                    text: line_text,
                    display_start_col: 0,
                }),
            },
            RoundFooterButton {
                round_start_block_index: round_start,
                start_col: button_start_col,
                end_col: button_start_col + button_width,
            },
        )
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

    pub(crate) fn visible_chat_lines(&self) -> Vec<Line<'static>> {
        self.rendered_chat_lines
            .iter()
            .enumerate()
            .map(|(line_index, line)| self.decorate_selection(line_index, line))
            .collect()
    }

    pub(crate) fn decorate_selection(
        &self,
        line_index: usize,
        line: &RenderedChatLine,
    ) -> Line<'static> {
        let mut display = line.display.clone();
        let Some(projection) = &line.copy_projection else {
            return display;
        };
        let line_width = line_display_width(&display);
        let Some((selection_start, selection_end)) =
            self.selection_display_range_for_line(line_index, line_width)
        else {
            return display;
        };
        let projection_end = projection.display_start_col + text_display_width(&projection.text);
        let highlight_start = selection_start.max(projection.display_start_col);
        let highlight_end = selection_end.min(projection_end);
        if highlight_start >= highlight_end {
            return display;
        }
        highlight_line_columns(&mut display, highlight_start, highlight_end);
        display
    }

    pub(crate) fn chat_card_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        match block.role {
            ChatRole::User => self.user_block_lines(block, content_width),
            ChatRole::Tool => self.tool_block_lines(block, content_width),
            ChatRole::ToolProgress => self.tool_progress_block_lines(block, content_width),
            ChatRole::Assistant => self.assistant_block_lines(block, content_width),
            ChatRole::Thinking => self.thinking_block_lines(block, content_width),
            ChatRole::System => self.system_block_lines(block, content_width),
        }
    }

    pub(crate) fn user_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let label_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let body_style = Style::default().fg(Color::White);

        lines.push(RenderedChatLine {
            display: Line::from(Span::styled("👤 User", label_style)),
            copy_projection: Some(CopyProjection {
                text: "User".to_string(),
                display_start_col: 0,
            }),
        });

        let content_lines = block
            .content
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect::<Vec<_>>();

        if !content_lines.is_empty() {
            helpers::push_block_header_gap(&mut lines);
        }

        for line in content_lines.iter() {
            for wrapped in wrap_line_visual(
                line.clone(),
                content_width.saturating_sub(CHAT_BLOCK_BODY_INDENT),
            ) {
                lines.push(self.decorate_chat_line(wrapped, body_style, CHAT_BLOCK_BODY_INDENT));
            }
        }

        lines
    }

    pub(crate) fn assistant_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let label_style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        lines.push(RenderedChatLine {
            display: Line::from(Span::styled("🤖 Assistant", label_style)),
            copy_projection: Some(CopyProjection {
                text: "Assistant".to_string(),
                display_start_col: 0,
            }),
        });
        let content_lines = render_markdown_lines(&block.content);

        if !content_lines.is_empty() {
            helpers::push_block_header_gap(&mut lines);
        }

        for line in content_lines.iter() {
            for wrapped in wrap_line_visual(
                line.clone(),
                content_width.saturating_sub(CHAT_BLOCK_BODY_INDENT),
            ) {
                let mut colored_spans = vec![];
                for span in &wrapped.spans {
                    let final_style = if span.style.fg.is_some() {
                        span.style
                    } else {
                        span.style.fg(Color::White)
                    };
                    colored_spans.push(Span::styled(span.content.to_string(), final_style));
                }
                lines.push(RenderedChatLine {
                    display: Line::from(
                        std::iter::once(Span::raw(" ".repeat(CHAT_BLOCK_BODY_INDENT)))
                            .chain(colored_spans.into_iter())
                            .collect::<Vec<_>>(),
                    ),
                    copy_projection: Some(CopyProjection {
                        text: line_plain_text(&wrapped),
                        display_start_col: CHAT_BLOCK_BODY_INDENT,
                    }),
                });
            }
        }

        lines
    }

    pub(crate) fn thinking_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let label_style = Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC);
        let body_style = Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::ITALIC | Modifier::DIM);

        if block.content.is_empty() {
            return lines;
        }

        lines.push(RenderedChatLine {
            display: Line::from(Span::styled("💭 Thinking", label_style)),
            copy_projection: Some(CopyProjection {
                text: "Thinking".to_string(),
                display_start_col: 0,
            }),
        });

        let content_lines = block
            .content
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect::<Vec<_>>();

        if !content_lines.is_empty() {
            helpers::push_block_header_gap(&mut lines);
        }

        for line in content_lines.iter() {
            for wrapped in wrap_line_visual(
                line.clone(),
                content_width.saturating_sub(CHAT_BLOCK_BODY_INDENT),
            ) {
                lines.push(self.decorate_chat_line(wrapped, body_style, CHAT_BLOCK_BODY_INDENT));
            }
        }

        lines
    }

    pub(crate) fn system_block_lines(
        &self,
        block: &ChatBlock,
        _content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let gear_style = Style::default().fg(Color::Yellow);

        let content_lines: Vec<&str> = block.content.lines().collect();
        if content_lines.is_empty() {
            return lines;
        }

        let is_welcome_card = content_lines[0].starts_with('╭');

        if is_welcome_card {
            for line in &content_lines {
                let display_line = Line::from(line.to_string());
                lines.push(RenderedChatLine {
                    display: display_line,
                    copy_projection: Some(CopyProjection {
                        text: line.to_string(),
                        display_start_col: 0,
                    }),
                });
            }
        } else {
            lines.push(RenderedChatLine {
                display: Line::from(vec![
                    Span::styled("⚙  ", gear_style),
                    Span::styled(
                        "System",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                copy_projection: Some(CopyProjection {
                    text: "⚙  System".to_string(),
                    display_start_col: 0,
                }),
            });
            helpers::push_block_header_gap(&mut lines);

            for line in &content_lines {
                let display_line = Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White),
                ));
                lines.push(RenderedChatLine {
                    display: display_line,
                    copy_projection: Some(CopyProjection {
                        text: line.to_string(),
                        display_start_col: 0,
                    }),
                });
            }
        }

        lines
    }

    pub(crate) fn decorate_chat_line(
        &self,
        line: Line<'static>,
        body_style: Style,
        indent: usize,
    ) -> RenderedChatLine {
        helpers::decorate_chat_line(line, body_style, indent)
    }
}
