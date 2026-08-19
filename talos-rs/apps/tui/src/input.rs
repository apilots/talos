use std::error::Error;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::session_picker::SessionPickerEvent;
use crate::types::{CommandSuggestion, PastedTextBlock};
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
        if key_event.code == KeyCode::Char('v')
            && key_event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
        {
            self.paste_clipboard_image();
            return Ok(false);
        }
        if key_event.modifiers.intersects(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('c') => return Ok(true),
                KeyCode::Char('o') => {
                    self.show_tool_details_hint();
                    return Ok(false);
                }
                KeyCode::Char('u') => {
                    self.reset_input();
                    return Ok(false);
                }
                _ => {}
            }
        }

        if self.pending_user_action.is_some() {
            if self
                .pending_user_action
                .as_ref()
                .is_some_and(crate::user_action::PendingActionState::is_editing_other)
            {
                match key_event.code {
                    KeyCode::Enter => self.submit_pending_user_action(),
                    KeyCode::Backspace => {
                        if let Some(action) = self.pending_user_action.as_mut() {
                            action.backspace();
                        }
                    }
                    KeyCode::Char(value)
                        if !key_event
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        if let Some(action) = self.pending_user_action.as_mut() {
                            action.insert_char(value);
                        }
                    }
                    KeyCode::Esc => {
                        self.show_status_hint(
                            "User action remains pending.",
                            Duration::from_secs(2),
                        );
                    }
                    _ => {}
                }
                return Ok(false);
            }
            match key_event.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(action) = self.pending_user_action.as_mut() {
                        action.move_selection(-1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(action) = self.pending_user_action.as_mut() {
                        action.move_selection(1);
                    }
                }
                KeyCode::Enter => self.submit_pending_user_action(),
                KeyCode::Backspace => {
                    if let Some(action) = self.pending_user_action.as_mut() {
                        action.backspace();
                    }
                }
                KeyCode::Char(value)
                    if !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if let Some(action) = self.pending_user_action.as_mut() {
                        action.insert_char(value);
                    }
                }
                KeyCode::Esc => {
                    self.show_status_hint("User action remains pending.", Duration::from_secs(2));
                }
                _ => {}
            }
            return Ok(false);
        }

        if self.permission_picker.is_some() {
            match key_event.code {
                KeyCode::Esc => self.permission_picker = None,
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(picker) = self.permission_picker.as_mut() {
                        picker.move_selection(-1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(picker) = self.permission_picker.as_mut() {
                        picker.move_selection(1);
                    }
                }
                KeyCode::Enter => self.confirm_permission_picker(),
                _ => {}
            }
            return Ok(false);
        }

        if self.model_picker.is_some() {
            match key_event.code {
                KeyCode::Esc => {
                    self.back_model_picker();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.navigate_model_picker(-1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.navigate_model_picker(1);
                }
                KeyCode::Enter => {
                    self.confirm_model_picker();
                }
                _ => {}
            }
            return Ok(false);
        }

        if let Some(picker) = self.session_picker.as_mut() {
            match picker.handle_key(key_event) {
                SessionPickerEvent::None => {}
                SessionPickerEvent::Cancel => {
                    self.session_picker = None;
                }
                SessionPickerEvent::Selected(session) => {
                    self.session_picker = None;
                    self.load_session_history(*session);
                }
            }
            return Ok(false);
        }

        if key_event.code == KeyCode::Char('q')
            && key_event.modifiers.intersects(KeyModifiers::CONTROL)
        {
            if self.selected_queue_message.is_some() {
                self.selected_queue_message = None;
            } else {
                self.select_message_queue();
            }
            return Ok(false);
        }

        if self.selected_queue_message.is_some() {
            match key_event.code {
                KeyCode::Esc => self.selected_queue_message = None,
                KeyCode::Up if key_event.modifiers.intersects(KeyModifiers::CONTROL) => {
                    self.reorder_selected_queued_message(-1).await;
                }
                KeyCode::Down if key_event.modifiers.intersects(KeyModifiers::CONTROL) => {
                    self.reorder_selected_queued_message(1).await;
                }
                KeyCode::Up | KeyCode::Char('k') => self.move_message_queue_selection(-1),
                KeyCode::Down | KeyCode::Char('j') => self.move_message_queue_selection(1),
                KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('d') => {
                    self.delete_selected_queued_message().await;
                }
                KeyCode::Char('e') => self.begin_selected_queue_edit(),
                KeyCode::Enter | KeyCode::Char('s') => {
                    self.start_or_steer_selected_queued_message().await;
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
                KeyCode::Up => {
                    self.skill_autocomplete_selected =
                        self.skill_autocomplete_selected.saturating_sub(1);
                    return Ok(false);
                }
                KeyCode::Down => {
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
                if self.editing_queue_message_id.take().is_some() {
                    self.reset_input();
                    return Ok(false);
                }
                if self.selected_queue_message.take().is_some() {
                    return Ok(false);
                }
                if self.in_flight {
                    self.cancel_active_turn().await;
                    return Ok(false);
                }
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
                self.move_cursor_left_across_pasted_image();
                self.refresh_skill_suggestions().await;
                Ok(false)
            }
            KeyCode::Right => {
                self.move_cursor_right_across_pasted_image();
                self.refresh_skill_suggestions().await;
                Ok(false)
            }
            KeyCode::Up => {
                self.navigate_message_history(-1);
                Ok(false)
            }
            KeyCode::Down => {
                self.navigate_message_history(1);
                Ok(false)
            }
            KeyCode::PageUp => Ok(false),
            KeyCode::PageDown => Ok(false),
            KeyCode::Home => {
                self.cursor_index = 0;
                self.refresh_skill_suggestions().await;
                Ok(false)
            }
            KeyCode::End => {
                self.cursor_index = self.input_buffer.chars().count();
                self.refresh_skill_suggestions().await;
                Ok(false)
            }
            KeyCode::Tab => {
                self.show_tool_details_hint();
                Ok(false)
            }
            KeyCode::BackTab => {
                self.show_tool_details_hint();
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

        self.move_cursor_out_of_pasted_image_marker();
        let idx = byte_index_at_char(&self.input_buffer, self.cursor_index);
        self.input_buffer.insert(idx, c);
        self.cursor_index += 1;
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        self.move_cursor_out_of_pasted_image_marker();
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

        if self.remove_pasted_image_before_cursor() {
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
        self.pasted_image_attachments.clear();
        self.pasted_image_sequence = 0;
        self.show_skill_autocomplete = false;
        self.skill_suggestions.clear();
        self.skill_autocomplete_selected = 0;
        self.selected_skills.clear();
    }

    pub(crate) fn should_store_as_pasted_block(text: &str) -> bool {
        text.chars().count() > PASTED_BLOCK_CHAR_THRESHOLD
    }

    pub(crate) fn has_composer_input(&self) -> bool {
        self.input_buffer.chars().any(|ch| !ch.is_whitespace())
    }

    pub(crate) fn is_command_input(&self) -> bool {
        self.pasted_blocks.is_empty()
            && self.pasted_image_attachments.is_empty()
            && self.input_buffer.trim().starts_with('/')
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
        let replacement = replace_skill_trigger(&self.input_buffer, &trigger, &skill.name);
        self.input_buffer = replacement.value;
        self.cursor_index = replacement.cursor_index;
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
        let active_turn = match client.get_active_turn(&session_id).await {
            Ok(active_turn) => active_turn,
            Err(err) => {
                self.push_system_notice(format!("Failed to read active turn: {err}"));
                self.push_right_event("cancel request failed".to_string());
                return;
            }
        };
        let Some(turn_id) = active_turn.turn_id else {
            self.show_status_hint("No active turn to cancel.", Duration::from_secs(2));
            return;
        };
        match client.cancel_active_turn(&session_id, &turn_id).await {
            Ok(()) => {
                self.push_right_event(format!("cancel requested for session {}", session_id));
                self.show_status_hint("Cancelling current turn...", Duration::from_secs(2));
            }
            Err(err) => {
                self.push_system_notice(format!("Failed to cancel turn: {}", err));
                self.push_right_event("cancel request failed".to_string());
            }
        }
    }
}

struct SkillTrigger {
    start: usize,
    end: usize,
    query: String,
}

struct SkillReplacement {
    value: String,
    cursor_index: usize,
}

fn active_skill_trigger(input: &str, cursor_index: usize) -> Option<SkillTrigger> {
    if cursor_index > input.chars().count() {
        return None;
    }
    let cursor_byte = byte_index_at_char(input, cursor_index);
    if input[cursor_byte..]
        .chars()
        .next()
        .is_some_and(is_skill_query_char)
    {
        return None;
    }
    let prefix = &input[..cursor_byte];
    let mut start = None;
    for (idx, ch) in prefix.char_indices().rev() {
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
    let query = &prefix[start + 1..];
    if !query.chars().all(is_skill_query_char) {
        return None;
    }
    Some(SkillTrigger {
        start,
        end: cursor_byte,
        query: query.to_string(),
    })
}

fn replace_skill_trigger(
    input: &str,
    trigger: &SkillTrigger,
    skill_name: &str,
) -> SkillReplacement {
    let before = &input[..trigger.start];
    let after = &input[trigger.end..];
    let prefix = if !before.is_empty()
        && before
            .chars()
            .next_back()
            .is_some_and(|ch| !ch.is_whitespace())
    {
        format!("{before} ")
    } else {
        before.to_string()
    };
    let token = format!("${skill_name}");
    let suffix = if after.is_empty() {
        " ".to_string()
    } else if after.chars().next().is_some_and(char::is_whitespace) {
        after.to_string()
    } else {
        format!(" {after}")
    };
    let cursor_index = prefix.chars().count()
        + token.chars().count()
        + usize::from(suffix.chars().next().is_some_and(char::is_whitespace));
    SkillReplacement {
        value: format!("{prefix}{token}{suffix}"),
        cursor_index,
    }
}

fn is_skill_query_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
}
