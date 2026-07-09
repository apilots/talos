use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::TuiApp;

const STATUS_LINE_HEIGHT: u16 = 1;
const QUEUE_PANEL_HEIGHT: u16 = 5;
const METRICS_PANEL_HEIGHT: u16 = 1;
const IDLE_TOP_GAP_HEIGHT: u16 = 2;
const MIN_LIVE_CHAT_PANEL_HEIGHT: u16 = 1;
const OVERLAY_VIEWPORT_HEIGHT: u16 = 18;

impl TuiApp {
    pub fn render_ui(&mut self, f: &mut Frame) {
        self.render_left_panel(f, f.area());
        if self.show_skill_autocomplete {
            self.render_skill_autocomplete(f, f.area());
        } else if self.show_command_autocomplete {
            self.render_command_autocomplete(f, f.area());
        } else if let Some(picker) = self.model_picker.as_ref() {
            let area = self.left_panel_area.unwrap_or(f.area());
            picker.render(f, area, self.input_area);
        } else if let Some(picker) = self.session_picker.as_mut() {
            picker.render(f, f.area());
        }
    }

    pub(crate) fn render_left_panel(&mut self, f: &mut Frame, area: Rect) {
        // Save left panel area for floater positioning
        self.left_panel_area = Some(area);

        if self.should_exit {
            self.render_exit_transcript_snapshot(f, area);
            return;
        }

        let left_inner = area;
        let show_queue_panel = !self.message_queue.is_empty();
        let chat_panel_height = self.inline_chat_panel_height(left_inner.width, left_inner.height);
        let show_chat_panel = chat_panel_height > 0;
        let top_gap_height = self.inline_top_gap_height();
        let input_panel_height = self.input_panel_height(left_inner.width);
        let metrics_line_height = self.metrics_panel_height();
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(top_gap_height),
                Constraint::Length(chat_panel_height),
                Constraint::Length(if show_queue_panel {
                    QUEUE_PANEL_HEIGHT
                } else {
                    0
                }),
                Constraint::Length(metrics_line_height),
                Constraint::Length(input_panel_height),
                Constraint::Length(STATUS_LINE_HEIGHT),
            ])
            .split(left_inner);

        if show_chat_panel {
            let chat_panel = Block::default();
            let chat_inner = chat_panel.inner(left_chunks[1]);
            f.render_widget(chat_panel, left_chunks[1]);

            let chat_content_rect = self.chat_content_rect(chat_inner);

            self.rebuild_chat_render_state(chat_content_rect);
            let visible_window = self.visible_chat_window();

            let conversation = Paragraph::new(visible_window.lines)
                .scroll((visible_window.local_scroll, 0))
                .style(Style::default().fg(Color::White));
            f.render_widget(conversation, chat_content_rect);
        } else {
            self.clear_inline_chat_viewport_state();
        }

        if show_queue_panel {
            let queue_lines =
                self.queue_panel_lines((left_chunks[2].height as usize).saturating_sub(1));
            let queue = Paragraph::new(queue_lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::Rgb(88, 92, 102))),
                )
                .style(Style::default().fg(Color::Gray).add_modifier(Modifier::DIM))
                .wrap(Wrap { trim: false });
            f.render_widget(queue, left_chunks[2]);
        }

        if metrics_line_height > 0 {
            let metrics_style = Style::default().fg(Color::White);
            if let Some(metrics_line) = self.turn_metrics_status_line(left_chunks[3].width as usize)
            {
                let metrics_row_rect = Rect {
                    y: left_chunks[3].y,
                    height: 1,
                    ..left_chunks[3]
                };
                let metrics_row = Paragraph::new(metrics_line)
                    .style(metrics_style)
                    .wrap(Wrap { trim: false });
                f.render_widget(metrics_row, metrics_row_rect);
            }
        }

        let input_panel = Block::default();
        let input_inner = input_panel.inner(left_chunks[4]);
        f.render_widget(input_panel, left_chunks[4]);
        self.input_area = Some(input_inner);

        let input_bg_style = Style::default().bg(Color::Rgb(72, 76, 88));
        f.render_widget(Block::default().style(input_bg_style), input_inner);
        let content_lines =
            self.input_lines(input_inner.width as usize, input_inner.height as usize);
        let input_row = Paragraph::new(content_lines).style(input_bg_style);
        f.render_widget(input_row, input_inner);

        let wait_style = if self.in_flight {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let wait_row = Paragraph::new(self.input_status_line(left_chunks[5].width as usize))
            .style(wait_style)
            .wrap(Wrap { trim: false });
        f.render_widget(wait_row, left_chunks[5]);
    }

    pub(crate) fn inline_viewport_height(&self, width: u16, terminal_height: u16) -> u16 {
        let target_height = if self.has_interaction_overlay() {
            OVERLAY_VIEWPORT_HEIGHT
        } else {
            self.inline_top_gap_height()
                .saturating_add(self.inline_chat_panel_height(width, terminal_height))
                .saturating_add(if self.message_queue.is_empty() {
                    0
                } else {
                    QUEUE_PANEL_HEIGHT
                })
                .saturating_add(self.metrics_panel_height())
                .saturating_add(self.input_panel_height(width))
                .saturating_add(STATUS_LINE_HEIGHT)
        };
        target_height.clamp(1, terminal_height.max(1))
    }

    fn input_panel_height(&self, width: u16) -> u16 {
        self.composer_line_count(width as usize)
            .saturating_add(2)
            .clamp(3, 8) as u16
    }

    fn metrics_panel_height(&self) -> u16 {
        if self.turn_metrics.is_some() {
            METRICS_PANEL_HEIGHT
        } else {
            0
        }
    }

    fn inline_top_gap_height(&self) -> u16 {
        if self.render_runtime.live_projection_lines().is_empty() && !self.is_agent_working() {
            IDLE_TOP_GAP_HEIGHT
        } else {
            0
        }
    }

    fn should_show_inline_chat_panel(&self) -> bool {
        self.render_runtime.store().blocks().is_empty()
            || self
                .render_runtime
                .store()
                .blocks()
                .iter()
                .any(|block| !block.terminal_synced)
            || !self.render_runtime.live_projection_lines().is_empty()
            || self.is_agent_working()
    }

    fn inline_chat_panel_height(&self, width: u16, terminal_height: u16) -> u16 {
        if !self.should_show_inline_chat_panel() {
            return 0;
        }
        if !self.render_runtime.store().blocks().is_empty()
            && self.render_runtime.live_projection_lines().is_empty()
            && !self.is_agent_working()
        {
            return 0;
        }
        let reserved_height = self
            .inline_top_gap_height()
            .saturating_add(self.input_panel_height(width))
            .saturating_add(STATUS_LINE_HEIGHT)
            .saturating_add(self.metrics_panel_height())
            .saturating_add(if self.message_queue.is_empty() {
                0
            } else {
                QUEUE_PANEL_HEIGHT
            });
        let available_height = terminal_height.saturating_sub(reserved_height);
        if available_height == 0 {
            return 0;
        }

        let live_line_count = self
            .render_runtime
            .live_projection_lines()
            .len()
            .min(u16::MAX as usize) as u16;
        let working_extra = if self.is_agent_working() { 2 } else { 0 };
        let target_height = live_line_count
            .saturating_add(working_extra)
            .max(MIN_LIVE_CHAT_PANEL_HEIGHT);
        target_height.min(available_height)
    }

    fn has_interaction_overlay(&self) -> bool {
        self.show_skill_autocomplete
            || self.show_command_autocomplete
            || self.model_picker.is_some()
            || self.session_picker.is_some()
    }

    fn clear_inline_chat_viewport_state(&mut self) {
        self.chat_viewport_height = 0;
        self.chat_content_rect = None;
        self.rendered_chat_lines.clear();
        self.rendered_chat_cache_key = None;
    }

    fn render_exit_transcript_snapshot(&mut self, f: &mut Frame, area: Rect) {
        let chat_panel = Block::default().style(Style::default().bg(Color::Rgb(33, 38, 49)));
        let chat_inner = chat_panel.inner(area);
        f.render_widget(chat_panel, area);

        let chat_content_rect = self.chat_content_rect(chat_inner);
        self.rebuild_chat_render_state(chat_content_rect);

        let visible_window = self.visible_chat_window();

        let conversation = Paragraph::new(visible_window.lines)
            .scroll((visible_window.local_scroll, 0))
            .style(Style::default().fg(Color::White));
        f.render_widget(conversation, chat_content_rect);
    }
}
