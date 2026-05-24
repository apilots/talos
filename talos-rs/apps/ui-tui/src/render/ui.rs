use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use crate::types::AppState;
use crate::utils::APP_VERSION;
use crate::TuiApp;

impl TuiApp {
    pub fn render_ui(&mut self, f: &mut Frame) {
        match self.current_state {
            AppState::History => {
                self.render_history_panel(f, f.area());
            }
            AppState::Chat => {
                self.render_left_panel(f, f.area());
                if self.show_command_autocomplete {
                    self.render_command_autocomplete(f, f.area());
                } else if self.show_model_picker {
                    self.render_model_picker(f, f.area());
                } else if self.show_sessions_picker {
                    self.render_sessions_picker(f, f.area());
                }
            }
        }
    }

    pub(crate) fn render_left_panel(&mut self, f: &mut Frame, area: Rect) {
        // Save left panel area for floater positioning
        self.left_panel_area = Some(area);

        let left_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(106, 111, 123)))
            .title(format!("Talos TUI ({})", APP_VERSION))
            .title_alignment(Alignment::Left)
            .style(Style::default().fg(Color::White));
        let left_inner = left_block.inner(area);
        f.render_widget(left_block, area);

        let show_queue_panel = !self.message_queue.is_empty();
        let input_panel_height = self
            .composer_line_count(left_inner.width as usize)
            .saturating_add(2)
            .clamp(3, 8) as u16;
        let metrics_line_height = u16::from(self.turn_metrics.is_some());
        let bottom_gap_height = 3;
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(if show_queue_panel { 5 } else { 0 }),
                Constraint::Length(metrics_line_height),
                Constraint::Length(input_panel_height),
                Constraint::Length(1),
                Constraint::Length(bottom_gap_height),
            ])
            .split(left_inner);

        let chat_panel = Block::default().style(Style::default().bg(Color::Rgb(33, 38, 49)));
        let chat_inner = chat_panel.inner(left_chunks[0]);
        f.render_widget(chat_panel, left_chunks[0]);

        let (chat_content_rect, chat_scrollbar_rect) = self.chat_content_columns(chat_inner);

        self.rebuild_chat_render_state(chat_content_rect);
        let visible_lines = self.visible_chat_lines();
        let viewport_height = self.chat_viewport_height;
        let total_lines = self.rendered_chat_lines.len();
        let max_scroll = total_lines.saturating_sub(viewport_height);
        if !self.chat_manual_scroll {
            self.chat_scroll = max_scroll as u16;
        } else {
            let max_scroll_u16 = max_scroll as u16;
            if self.chat_scroll >= max_scroll_u16 {
                self.chat_scroll = max_scroll_u16;
                self.chat_manual_scroll = false;
            }
        }

        let conversation = Paragraph::new(visible_lines)
            .scroll((self.chat_scroll, 0))
            .style(Style::default().fg(Color::White));
        f.render_widget(conversation, chat_content_rect);

        if chat_scrollbar_rect.width > 0 && chat_scrollbar_rect.height > 0 {
            f.render_widget(
                Block::default().style(Style::default().bg(Color::Rgb(18, 21, 28))),
                chat_scrollbar_rect,
            );
        }

        if max_scroll > 0 {
            let mut state = ScrollbarState::new(max_scroll).position(self.chat_scroll as usize);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .style(Style::default().fg(Color::Rgb(58, 63, 74)))
                .thumb_style(Style::default().fg(Color::Rgb(89, 96, 110)));
            f.render_stateful_widget(scrollbar, chat_scrollbar_rect, &mut state);
        }

        if show_queue_panel {
            let queue_lines =
                self.queue_panel_lines((left_chunks[1].height as usize).saturating_sub(1));
            let queue = Paragraph::new(queue_lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::Rgb(88, 92, 102))),
                )
                .style(Style::default().fg(Color::Gray).add_modifier(Modifier::DIM))
                .wrap(Wrap { trim: false });
            f.render_widget(queue, left_chunks[1]);
        }

        if metrics_line_height > 0 {
            let metrics_style = Style::default().bg(Color::Rgb(36, 39, 46)).fg(Color::White);
            f.render_widget(
                Block::default().style(Style::default().bg(Color::Rgb(36, 39, 46))),
                left_chunks[2],
            );
            if let Some(metrics_line) = self.turn_metrics_status_line(left_chunks[2].width as usize)
            {
                let metrics_row = Paragraph::new(metrics_line)
                    .style(metrics_style)
                    .wrap(Wrap { trim: false });
                f.render_widget(metrics_row, left_chunks[2]);
            }
        }

        let input_panel = Block::default();
        let input_inner = input_panel.inner(left_chunks[3]);
        f.render_widget(input_panel, left_chunks[3]);
        self.input_area = Some(input_inner);

        let input_bg_style = Style::default().bg(Color::Rgb(72, 76, 88));
        f.render_widget(Block::default().style(input_bg_style), input_inner);
        let content_lines =
            self.input_lines(input_inner.width as usize, input_inner.height as usize);
        let input_row = Paragraph::new(content_lines).style(input_bg_style);
        f.render_widget(input_row, input_inner);

        let wait_style = if self.in_flight {
            Style::default().bg(Color::Rgb(36, 39, 46)).fg(Color::Cyan)
        } else {
            Style::default().bg(Color::Rgb(36, 39, 46)).fg(Color::White)
        };
        let status_block = Block::default().style(Style::default().bg(Color::Rgb(36, 39, 46)));
        f.render_widget(status_block, left_chunks[4]);
        let wait_row = Paragraph::new(self.input_status_line(left_chunks[4].width as usize))
            .style(wait_style)
            .wrap(Wrap { trim: false });
        f.render_widget(wait_row, left_chunks[4]);

        let bottom_gap = Block::default().style(Style::default().bg(Color::Rgb(33, 38, 49)));
        f.render_widget(bottom_gap, left_chunks[5]);
    }
}
