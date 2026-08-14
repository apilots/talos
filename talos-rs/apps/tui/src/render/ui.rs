use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::render::{composer, frame::RenderFrame};
use crate::TuiApp;

const QUEUE_PANEL_HEIGHT: u16 = 5;
const IDLE_TOP_GAP_HEIGHT: u16 = 2;
const MIN_LIVE_CHAT_PANEL_HEIGHT: u16 = 1;
const OVERLAY_VIEWPORT_HEIGHT: u16 = 18;
const TRANSCRIPT_COMPOSER_GAP_HEIGHT: u16 = 1;

impl TuiApp {
    pub(crate) fn render_ui(&mut self, f: &mut impl RenderFrame) {
        if let Some(picker) = self.session_picker.as_mut() {
            picker.render(f, f.area());
            return;
        }

        self.render_left_panel(f, f.area());
        if let Some(action) = self.pending_user_action.as_ref() {
            action.render(f, f.area());
        } else if let Some(picker) = self.permission_picker.as_ref() {
            picker.render(f, f.area());
        } else if self.show_skill_autocomplete {
            self.render_skill_autocomplete(f, f.area());
        } else if self.show_command_autocomplete {
            self.render_command_autocomplete(f, f.area());
        } else if let Some(picker) = self.model_picker.as_ref() {
            let area = self.left_panel_area.unwrap_or(f.area());
            picker.render(f, area, self.input_area);
        }
    }

    pub(crate) fn render_left_panel(&mut self, f: &mut impl RenderFrame, area: Rect) {
        // Save left panel area for floater positioning
        self.left_panel_area = Some(area);

        if self.should_exit {
            self.render_exit_transcript_snapshot(f, area);
            return;
        }

        let left_inner = area;
        let show_queue_panel = !self.message_queue.is_empty();
        let composer_height = composer::height(self, left_inner.width);
        let transcript_composer_gap_height = self.transcript_composer_gap_height();
        let chat_panel_height = self.inline_chat_panel_height(
            left_inner.width,
            left_inner.height,
            composer_height,
            transcript_composer_gap_height,
        );
        let show_chat_panel = chat_panel_height > 0;
        let top_gap_height = self.inline_top_gap_height();
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
                Constraint::Length(transcript_composer_gap_height),
                Constraint::Length(composer_height),
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

        composer::render(self, f, left_chunks[4]);
    }

    pub(crate) fn inline_viewport_height(&self, terminal_width: u16, terminal_height: u16) -> u16 {
        let composer_height = composer::height(self, terminal_width);
        let target_height = if self.uses_transient_fullscreen_surface() {
            terminal_height
        } else if self.has_interaction_overlay() {
            OVERLAY_VIEWPORT_HEIGHT
        } else {
            let transcript_composer_gap_height = self.transcript_composer_gap_height();
            self.inline_top_gap_height()
                .saturating_add(self.inline_chat_panel_height(
                    terminal_width,
                    terminal_height,
                    composer_height,
                    transcript_composer_gap_height,
                ))
                .saturating_add(if self.message_queue.is_empty() {
                    0
                } else {
                    QUEUE_PANEL_HEIGHT
                })
                .saturating_add(transcript_composer_gap_height)
                .saturating_add(composer_height)
        };
        target_height.clamp(1, terminal_height.max(1))
    }

    pub(crate) fn uses_transient_fullscreen_surface(&self) -> bool {
        self.session_picker.is_some()
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

    fn transcript_composer_gap_height(&self) -> u16 {
        if self.should_show_inline_chat_panel() {
            TRANSCRIPT_COMPOSER_GAP_HEIGHT
        } else {
            0
        }
    }

    fn inline_chat_panel_height(
        &self,
        terminal_width: u16,
        terminal_height: u16,
        composer_height: u16,
        transcript_composer_gap_height: u16,
    ) -> u16 {
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
            .saturating_add(composer_height)
            .saturating_add(transcript_composer_gap_height)
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
            .projected_live_chat_line_count(terminal_width)
            .min(u16::MAX as usize) as u16;
        let target_height = live_line_count.max(MIN_LIVE_CHAT_PANEL_HEIGHT);
        target_height.min(available_height)
    }

    fn has_interaction_overlay(&self) -> bool {
        self.show_skill_autocomplete
            || self.show_command_autocomplete
            || self.model_picker.is_some()
            || self.permission_picker.is_some()
            || self.pending_user_action.is_some()
            || self.session_picker.is_some()
    }

    fn clear_inline_chat_viewport_state(&mut self) {
        self.chat_viewport_height = 0;
        self.chat_content_rect = None;
        self.rendered_chat_lines.clear();
        self.rendered_chat_cache_key = None;
    }

    fn render_exit_transcript_snapshot(&mut self, f: &mut impl RenderFrame, area: Rect) {
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
