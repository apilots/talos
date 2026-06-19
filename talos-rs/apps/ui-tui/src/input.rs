use std::error::Error;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::types::{
    AppState, ChatRole, ChatSelection, CommandSuggestion, PastedTextBlock, SelectionPoint,
};
use crate::utils::byte_index_at_char;
use crate::{TuiApp, PASTED_BLOCK_CHAR_THRESHOLD};

impl TuiApp {
    pub fn handle_paste(&mut self, text: String) {
        if text.is_empty() {
            return;
        }

        let text = text.replace('\r', "");
        self.show_command_autocomplete = false;
        self.command_input_buffer.clear();
        if Self::should_store_as_pasted_block(&text) {
            let marker = format!(
                "[Pasted text #{} · {} lines]",
                self.pasted_blocks.len() + 1,
                text.lines().count().max(1)
            );
            self.insert_text(&marker);
            self.pasted_blocks.push(PastedTextBlock::new(marker, text));
            self.selected_pasted_block = Some(self.pasted_blocks.len().saturating_sub(1));
        } else {
            self.insert_text(&text);
        }
    }

    pub async fn handle_input(&mut self, key_event: KeyEvent) -> Result<bool, Box<dyn Error>> {
        if key_event.modifiers.intersects(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('c') => return Ok(true),
                KeyCode::Char('o') => {
                    self.toggle_last_tool_block();
                    self.rebuild_chat_render_state_if_rect();
                    return Ok(false);
                }
                KeyCode::Char('u') => {
                    self.reset_input();
                    return Ok(false);
                }
                KeyCode::Char('y') => {
                    self.copy_chat_to_clipboard();
                    return Ok(false);
                }
                _ => {}
            }
        }

        // History state navigation
        if self.current_state == AppState::History {
            match key_event.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.current_state = AppState::Chat;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.history_selected = self.history_selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.history_sessions.len().saturating_sub(1);
                    if self.history_selected < max {
                        self.history_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.switch_to_history_session();
                }
                KeyCode::Char('d') => {
                    self.delete_history_session();
                }
                _ => {}
            }
            return Ok(false);
        }

        // Model picker navigation
        if self.show_model_picker {
            match key_event.code {
                KeyCode::Esc => {
                    self.show_model_picker = false;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.model_picker_selected = self.model_picker_selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.available_models.len().saturating_sub(1);
                    if self.model_picker_selected < max {
                        self.model_picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.confirm_model_picker();
                }
                _ => {}
            }
            return Ok(false);
        }

        // Sessions picker navigation
        if self.show_sessions_picker {
            match key_event.code {
                KeyCode::Esc => {
                    self.show_sessions_picker = false;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.sessions_picker_selected = self.sessions_picker_selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.history_sessions.len().saturating_sub(1);
                    if self.sessions_picker_selected < max {
                        self.sessions_picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.switch_to_sessions_picker();
                }
                KeyCode::Char(c) if c.is_numeric() => {
                    let index = (c.to_digit(10).unwrap_or(0) as usize).saturating_sub(1);
                    if index < self.history_sessions.len() {
                        self.sessions_picker_selected = index;
                        self.switch_to_sessions_picker();
                    }
                }
                _ => {}
            }
            return Ok(false);
        }

        // Skill autocomplete navigation
        if self.show_skill_autocomplete {
            match key_event.code {
                KeyCode::Esc => {
                    self.show_skill_autocomplete = false;
                    self.skill_suggestions.clear();
                    return Ok(false);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.skill_autocomplete_selected =
                        self.skill_autocomplete_selected.saturating_sub(1);
                    return Ok(false);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.skill_suggestions.len().saturating_sub(1);
                    if self.skill_autocomplete_selected < max {
                        self.skill_autocomplete_selected += 1;
                    }
                    return Ok(false);
                }
                KeyCode::Enter | KeyCode::Tab => {
                    self.select_current_skill_suggestion();
                    return Ok(false);
                }
                KeyCode::Backspace => {
                    self.handle_backspace();
                    self.refresh_skill_suggestions().await;
                    return Ok(false);
                }
                KeyCode::Char(c)
                    if !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.insert_char(c);
                    self.refresh_skill_suggestions().await;
                    return Ok(false);
                }
                _ => {}
            }
            return Ok(false);
        }

        // Command autocomplete navigation
        if self.show_command_autocomplete {
            match key_event.code {
                KeyCode::Esc => {
                    self.show_command_autocomplete = false;
                    self.command_input_buffer.clear();
                    return Ok(false);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.command_autocomplete_selected =
                        self.command_autocomplete_selected.saturating_sub(1);
                    return Ok(false);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.command_suggestions.len().saturating_sub(1);
                    if self.command_autocomplete_selected < max {
                        self.command_autocomplete_selected += 1;
                    }
                    return Ok(false);
                }
                KeyCode::Enter => {
                    if let Some(cmd) = self
                        .command_suggestions
                        .get(self.command_autocomplete_selected)
                    {
                        self.input_buffer = cmd.insert_text.clone();
                        self.cursor_index = self.input_buffer.chars().count();
                        self.show_command_autocomplete = false;
                        self.command_input_buffer.clear();
                    }
                    return Ok(false);
                }
                KeyCode::Backspace => {
                    self.command_input_buffer.pop();
                    if self.command_input_buffer.is_empty() {
                        self.input_buffer = "/".to_string();
                        self.cursor_index = 1;
                        self.show_command_autocomplete = false;
                    } else {
                        self.sync_command_input_to_main_buffer();
                        self.update_command_suggestions();
                    }
                    return Ok(false);
                }
                KeyCode::Char(c)
                    if !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.command_input_buffer.push(c);
                    self.sync_command_input_to_main_buffer();
                    self.update_command_suggestions();
                    return Ok(false);
                }
                _ => {}
            }
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                if self.in_flight {
                    self.cancel_active_turn().await;
                    return Ok(false);
                }
                self.chat_selection = None;
                self.selected_pasted_block = None;
                Ok(false)
            }
            KeyCode::Enter => {
                let should_quit = self.submit_input().await?;
                Ok(should_quit)
            }
            KeyCode::Backspace => {
                self.handle_backspace();
                self.refresh_skill_suggestions().await;
                Ok(false)
            }
            KeyCode::Left => {
                self.cursor_index = self.cursor_index.saturating_sub(1);
                Ok(false)
            }
            KeyCode::Right => {
                let count = self.input_buffer.chars().count();
                if self.cursor_index < count {
                    self.cursor_index += 1;
                }
                Ok(false)
            }
            KeyCode::Up => {
                self.navigate_message_history(-1);
                Ok(false)
            }
            KeyCode::Down => {
                if self.message_history_index.is_some() {
                    self.navigate_message_history(1);
                }
                Ok(false)
            }
            KeyCode::PageUp => {
                self.scroll_chat_up(8);
                Ok(false)
            }
            KeyCode::PageDown => {
                self.scroll_chat_down(8);
                Ok(false)
            }
            KeyCode::Home => {
                self.scroll_chat_top();
                Ok(false)
            }
            KeyCode::End => {
                self.scroll_chat_bottom();
                Ok(false)
            }
            KeyCode::Tab => {
                self.toggle_last_tool_block();
                self.rebuild_chat_render_state_if_rect();
                Ok(false)
            }
            KeyCode::BackTab => {
                self.toggle_last_tool_block();
                self.rebuild_chat_render_state_if_rect();
                Ok(false)
            }
            KeyCode::Char(c) => {
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    self.insert_char(c);
                    self.refresh_skill_suggestions().await;
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub(crate) fn sync_command_input_to_main_buffer(&mut self) {
        self.input_buffer = if self.command_input_buffer.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", self.command_input_buffer)
        };
        self.cursor_index = self.input_buffer.chars().count();
    }

    pub(crate) fn insert_char(&mut self, c: char) {
        // Trigger command autocomplete when typing /
        if c == '/' && self.cursor_index == 0 && self.input_buffer.is_empty() {
            self.show_command_autocomplete = true;
            self.command_suggestions = CommandSuggestion::all();
            self.command_autocomplete_selected = 0;
            self.command_input_buffer.clear();
            self.sync_command_input_to_main_buffer();
            return;
        }

        let idx = byte_index_at_char(&self.input_buffer, self.cursor_index);
        self.input_buffer.insert(idx, c);
        self.cursor_index += 1;
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        let idx = byte_index_at_char(&self.input_buffer, self.cursor_index);
        self.input_buffer.insert_str(idx, text);
        self.cursor_index += text.chars().count();
    }

    pub(crate) fn update_command_suggestions(&mut self) {
        self.command_suggestions = CommandSuggestion::filter(&self.command_input_buffer);
        self.command_autocomplete_selected = 0;
    }

    pub(crate) fn handle_backspace(&mut self) {
        if self.cursor_index == 0 {
            return;
        }

        let current = byte_index_at_char(&self.input_buffer, self.cursor_index);
        let previous = byte_index_at_char(&self.input_buffer, self.cursor_index - 1);
        self.input_buffer.drain(previous..current);
        self.cursor_index -= 1;
    }

    pub(crate) fn reset_input(&mut self) {
        self.input_buffer.clear();
        self.cursor_index = 0;
        self.pasted_blocks.clear();
        self.selected_pasted_block = None;
        self.show_skill_autocomplete = false;
        self.skill_suggestions.clear();
        self.skill_autocomplete_selected = 0;
        self.selected_skills.clear();
    }

    pub(crate) fn restore_input_state(
        &mut self,
        input_buffer: String,
        pasted_blocks: Vec<PastedTextBlock>,
    ) {
        self.input_buffer = input_buffer;
        self.cursor_index = self.input_buffer.chars().count();
        self.pasted_blocks = pasted_blocks;
        self.selected_pasted_block = if self.pasted_blocks.is_empty() {
            None
        } else {
            Some(self.pasted_blocks.len().saturating_sub(1))
        };
    }

    pub(crate) fn should_store_as_pasted_block(text: &str) -> bool {
        text.chars().count() > PASTED_BLOCK_CHAR_THRESHOLD
    }

    pub(crate) fn has_composer_input(&self) -> bool {
        self.input_buffer.chars().any(|ch| !ch.is_whitespace())
    }

    pub(crate) fn is_command_input(&self) -> bool {
        self.pasted_blocks.is_empty() && self.input_buffer.trim().starts_with('/')
    }

    pub(crate) fn compose_user_input_text(&self) -> String {
        let mut composed = self.input_buffer.clone();
        for block in &self.pasted_blocks {
            composed = composed.replacen(&block.marker, &block.text, 1);
        }
        composed
    }

    pub(crate) async fn refresh_skill_suggestions(&mut self) {
        let Some(trigger) = active_skill_trigger(&self.input_buffer, self.cursor_index) else {
            self.show_skill_autocomplete = false;
            self.skill_suggestions.clear();
            self.skill_autocomplete_selected = 0;
            self.selected_skills
                .retain(|skill| self.input_buffer.contains(&format!("${}", skill.name)));
            return;
        };
        if self.show_command_autocomplete {
            return;
        }
        let Some(client) = self.gateway_client.clone() else {
            return;
        };
        match client.suggest_skills(&trigger.query).await {
            Ok(skills) => {
                self.skill_suggestions = skills;
                self.skill_autocomplete_selected = 0;
                self.show_skill_autocomplete = !self.skill_suggestions.is_empty();
            }
            Err(err) => {
                self.show_skill_autocomplete = false;
                self.skill_suggestions.clear();
                self.push_right_event(format!("skill suggest failed: {}", err));
            }
        }
    }

    pub(crate) fn select_current_skill_suggestion(&mut self) {
        let Some(trigger) = active_skill_trigger(&self.input_buffer, self.cursor_index) else {
            self.show_skill_autocomplete = false;
            return;
        };
        let Some(skill) = self
            .skill_suggestions
            .get(self.skill_autocomplete_selected)
            .cloned()
            .or_else(|| self.skill_suggestions.first().cloned())
        else {
            self.show_skill_autocomplete = false;
            return;
        };
        self.input_buffer
            .replace_range(trigger.start..trigger.end, &format!("${} ", skill.name));
        self.cursor_index = self.input_buffer.chars().count();
        if !self
            .selected_skills
            .iter()
            .any(|existing| existing.path == skill.path)
        {
            self.selected_skills.push(skill);
        }
        self.show_skill_autocomplete = false;
        self.skill_suggestions.clear();
        self.skill_autocomplete_selected = 0;
    }

    pub(crate) fn show_status_hint(&mut self, message: impl Into<String>, ttl: Duration) {
        self.status_hint = Some((message.into(), Instant::now() + ttl));
    }

    pub(crate) async fn cancel_active_turn(&mut self) {
        let Some(session_id) = self.session_id.clone() else {
            self.show_status_hint("No active session to cancel.", Duration::from_secs(2));
            return;
        };
        let Some(client) = self.gateway_client.clone() else {
            self.show_status_hint("Gateway client unavailable.", Duration::from_secs(2));
            return;
        };
        match client.cancel_active_turn(&session_id).await {
            Ok(()) => {
                self.push_right_event(format!("cancel requested for session {}", session_id));
                self.show_status_hint("Cancelling current turn...", Duration::from_secs(2));
            }
            Err(err) => {
                self.push_block(ChatRole::System, format!("Failed to cancel turn: {}", err));
                self.push_right_event("cancel request failed".to_string());
            }
        }
    }

    pub(crate) fn scroll_chat_up(&mut self, lines: u16) {
        self.chat_manual_scroll = true;
        self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    pub(crate) fn scroll_chat_down(&mut self, lines: u16) {
        self.chat_manual_scroll = true;
        self.chat_scroll = self.chat_scroll.saturating_add(lines);
    }

    pub(crate) fn scroll_chat_top(&mut self) {
        self.chat_manual_scroll = true;
        self.chat_scroll = 0;
    }

    pub(crate) fn scroll_chat_bottom(&mut self) {
        self.chat_manual_scroll = false;
    }

    pub(crate) fn request_follow_bottom(&mut self) {
        if !self.chat_manual_scroll {
            self.chat_scroll = u16::MAX;
        }
    }

    pub(crate) fn handle_mouse(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(point) = self.selection_point_from_mouse(mouse_event) {
                    if let Some(button) = self.rendered_round_footer_buttons.get(&point.line_index)
                    {
                        if point.column >= button.start_col && point.column < button.end_col {
                            self.chat_selection = None;
                            self.copy_round_for_start_block(button.round_start_block_index);
                            return;
                        }
                    }
                    if self
                        .rendered_round_footer_buttons
                        .contains_key(&point.line_index)
                    {
                        self.chat_selection = None;
                        return;
                    }
                    self.chat_selection = Some(ChatSelection {
                        anchor: point,
                        focus: point,
                    });
                } else {
                    self.chat_selection = None;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let (Some(point), Some(selection)) = (
                    self.selection_point_from_mouse(mouse_event),
                    self.chat_selection.as_mut(),
                ) {
                    selection.focus = point;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let (Some(point), Some(selection)) = (
                    self.selection_point_from_mouse(mouse_event),
                    self.chat_selection.as_mut(),
                ) {
                    selection.focus = point;
                }
                if self.chat_selection.is_some() {
                    self.copy_selection_to_clipboard();
                }
            }
            MouseEventKind::ScrollUp => self.scroll_chat_up(3),
            MouseEventKind::ScrollDown => self.scroll_chat_down(3),
            _ => {}
        }
    }

    pub(crate) fn selection_point_from_mouse(
        &self,
        mouse_event: MouseEvent,
    ) -> Option<SelectionPoint> {
        let rect = self.chat_content_rect?;
        if rect.width == 0 || rect.height == 0 {
            return None;
        }
        if mouse_event.column < rect.x
            || mouse_event.column >= rect.x.saturating_add(rect.width)
            || mouse_event.row < rect.y
            || mouse_event.row >= rect.y.saturating_add(rect.height)
        {
            return None;
        }

        let row_offset = mouse_event.row.saturating_sub(rect.y) as usize;
        let line_index = self.chat_scroll as usize + row_offset;
        if line_index >= self.rendered_chat_lines.len() {
            return None;
        }

        Some(SelectionPoint {
            line_index,
            column: mouse_event.column.saturating_sub(rect.x) as usize,
        })
    }
}

struct SkillTrigger {
    start: usize,
    end: usize,
    query: String,
}

fn active_skill_trigger(input: &str, cursor_index: usize) -> Option<SkillTrigger> {
    if cursor_index != input.chars().count() {
        return None;
    }
    let mut start = None;
    for (idx, ch) in input.char_indices().rev() {
        if ch == '$' {
            start = Some(idx);
            break;
        }
        if ch.is_whitespace() {
            return None;
        }
    }
    let start = start?;
    if start > 0 {
        let previous = input[..start].chars().next_back();
        if previous.is_some_and(|ch| !ch.is_whitespace()) {
            return None;
        }
    }
    let query = &input[start + 1..];
    if !query
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        return None;
    }
    Some(SkillTrigger {
        start,
        end: input.len(),
        query: query.to_string(),
    })
}
