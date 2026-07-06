use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::render::helpers;
use crate::status::render_status_card_lines;
use crate::types::{ChatBlock, ChatRole, CopyProjection, RenderedChatLine, RoundFooterButton};
use crate::utils::{
    highlight_line_columns, line_display_width, line_plain_text, text_display_width,
    wrap_line_visual,
};
use crate::{TuiApp, COPY_ROUND_BUTTON_TEXT};

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
        if self.chat_blocks[round_start..round_end_exclusive]
            .iter()
            .any(|block| block.role == ChatRole::Status)
        {
            return false;
        }
        if round_end_exclusive < self.chat_blocks.len() {
            return true;
        }
        !self.in_flight
            && self.chat_blocks[round_start..round_end_exclusive]
                .iter()
                .any(|block| matches!(block.role, ChatRole::FinalAnswer | ChatRole::System))
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
            ChatRole::Commentary => self.commentary_block_lines(block, content_width),
            ChatRole::Thinking => self.thinking_block_lines(block, content_width),
            ChatRole::FinalAnswer => self.final_answer_block_lines(block, content_width),
            ChatRole::System => self.system_block_lines(block, content_width),
            ChatRole::Status => self.status_block_lines(block),
        }
    }

    pub(crate) fn user_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let body_style = if block.content.trim_start().starts_with('/') {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        bullet_transcript_lines(
            &block.content,
            content_width,
            Style::default().fg(Color::Gray),
            body_style,
            None,
        )
    }

    pub(crate) fn commentary_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let body_style = Style::default().fg(Color::White);
        bullet_transcript_lines(
            &block.content,
            content_width,
            Style::default().fg(Color::Gray),
            body_style,
            None,
        )
    }

    pub(crate) fn final_answer_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let body_style = Style::default().fg(Color::White);
        plain_transcript_lines(&block.content, content_width, body_style)
    }

    pub(crate) fn thinking_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let body_style = Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::ITALIC | Modifier::DIM);

        if block.content.is_empty() {
            return Vec::new();
        }

        if block.expanded {
            let mut lines = bullet_transcript_lines(
                &block.content,
                content_width,
                Style::default().fg(Color::Gray),
                body_style,
                None,
            );
            lines.push(RenderedChatLine {
                display: Line::from(Span::styled(
                    "  (ctrl+o to collapse)",
                    Style::default().fg(Color::DarkGray),
                )),
                copy_projection: Some(CopyProjection {
                    text: "(ctrl+o to collapse)".to_string(),
                    display_start_col: 2,
                }),
            });
            lines
        } else {
            let preview = block
                .content
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
                None,
            )
        }
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

    pub(crate) fn status_block_lines(&self, block: &ChatBlock) -> Vec<RenderedChatLine> {
        render_status_card_lines(&block.content)
    }
}

fn bullet_transcript_lines(
    content: &str,
    content_width: usize,
    bullet_style: Style,
    body_style: Style,
    empty_text: Option<&str>,
) -> Vec<RenderedChatLine> {
    let mut lines = Vec::new();
    let source = if content.is_empty() {
        empty_text.unwrap_or("")
    } else {
        content
    };
    let mut first_content_line = true;

    for raw_line in source.lines() {
        let prefix = if first_content_line { "● " } else { "  " };
        let prefix_style = if first_content_line {
            bullet_style
        } else {
            Style::default()
        };
        let prefix_width = text_display_width(prefix);
        first_content_line = false;

        let wrapped_lines = wrap_line_visual(
            Line::from(Span::styled(raw_line.to_string(), body_style)),
            content_width.saturating_sub(prefix_width),
        );
        for (index, wrapped) in wrapped_lines.into_iter().enumerate() {
            let actual_prefix = if index == 0 { prefix } else { "  " };
            let actual_prefix_width = text_display_width(actual_prefix);
            let plain = line_plain_text(&wrapped);
            let display = Line::from(
                std::iter::once(Span::styled(actual_prefix.to_string(), prefix_style))
                    .chain(wrapped.spans)
                    .collect::<Vec<_>>(),
            );
            lines.push(RenderedChatLine {
                display,
                copy_projection: Some(CopyProjection {
                    text: plain,
                    display_start_col: actual_prefix_width,
                }),
            });
        }
    }

    if lines.is_empty() {
        lines.push(RenderedChatLine {
            display: Line::from(vec![
                Span::styled("● ", bullet_style),
                Span::styled(String::new(), body_style),
            ]),
            copy_projection: Some(CopyProjection {
                text: String::new(),
                display_start_col: 2,
            }),
        });
    }

    lines
}

fn prefix_transcript_lines(
    content: &str,
    content_width: usize,
    prefix: &str,
    prefix_style: Style,
    body_style: Style,
    empty_text: Option<&str>,
) -> Vec<RenderedChatLine> {
    let mut lines = Vec::new();
    let source = if content.is_empty() {
        empty_text.unwrap_or("")
    } else {
        content
    };
    let mut first_content_line = true;

    for raw_line in source.lines() {
        let actual_prefix = if first_content_line { prefix } else { "  " };
        let actual_prefix_style = if first_content_line {
            prefix_style
        } else {
            Style::default()
        };
        first_content_line = false;
        let prefix_width = text_display_width(actual_prefix);
        let wrapped_lines = wrap_line_visual(
            Line::from(Span::styled(raw_line.to_string(), body_style)),
            content_width.saturating_sub(prefix_width),
        );
        for (index, wrapped) in wrapped_lines.into_iter().enumerate() {
            let line_prefix = if index == 0 { actual_prefix } else { "  " };
            let line_prefix_width = text_display_width(line_prefix);
            let plain = line_plain_text(&wrapped);
            let display = Line::from(
                std::iter::once(Span::styled(line_prefix.to_string(), actual_prefix_style))
                    .chain(wrapped.spans)
                    .collect::<Vec<_>>(),
            );
            lines.push(RenderedChatLine {
                display,
                copy_projection: Some(CopyProjection {
                    text: plain,
                    display_start_col: line_prefix_width,
                }),
            });
        }
    }

    lines
}

fn plain_transcript_lines(
    content: &str,
    content_width: usize,
    body_style: Style,
) -> Vec<RenderedChatLine> {
    let mut lines = Vec::new();
    for raw_line in content.lines() {
        let wrapped_lines = wrap_line_visual(
            Line::from(Span::styled(raw_line.to_string(), body_style)),
            content_width,
        );
        for wrapped in wrapped_lines {
            let plain = line_plain_text(&wrapped);
            lines.push(RenderedChatLine {
                display: wrapped,
                copy_projection: Some(CopyProjection {
                    text: plain,
                    display_start_col: 0,
                }),
            });
        }
    }
    if lines.is_empty() {
        lines.push(RenderedChatLine {
            display: Line::from(String::new()),
            copy_projection: Some(CopyProjection {
                text: String::new(),
                display_start_col: 0,
            }),
        });
    }
    lines
}
