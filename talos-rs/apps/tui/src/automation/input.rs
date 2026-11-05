//! Keyboard actions for the existing composer task surface.
use crate::TuiApp;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use talos_error::TalosResult;
impl TuiApp {
    pub(crate) async fn handle_automation_input(
        &mut self,
        key_event: KeyEvent,
    ) -> TalosResult<bool> {
        if key_event.modifiers.contains(KeyModifiers::ALT)
            && key_event.code == KeyCode::Backspace
            && self.automation_edit.is_none()
        {
            self.selected_skills
                .retain(|skill| !matches!(skill.name.as_str(), "goal" | "loop"));
            self.show_status_hint("Task mode cleared; prompt kept.", Duration::from_secs(2));
            return Ok(true);
        }
        if self.automation_edit.is_some() {
            match key_event.code {
                KeyCode::Esc => {
                    self.automation_edit = None;
                    self.reset_input();
                    return Ok(true);
                }
                KeyCode::Tab => {
                    if let Some(edit) = self.automation_edit.as_mut() {
                        if let Some(schedule) = &edit.schedule {
                            edit.schedule = Some(match schedule {
                                talos_sdk::LoopSchedule::ModelDecided {} => {
                                    talos_sdk::LoopSchedule::FixedInterval { minutes: 1 }
                                }
                                _ => talos_sdk::LoopSchedule::ModelDecided {},
                            });
                        }
                    }
                    return Ok(true);
                }
                KeyCode::Up | KeyCode::Down
                    if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    if let Some(edit) = self.automation_edit.as_mut() {
                        if let Some(talos_sdk::LoopSchedule::FixedInterval { minutes }) =
                            &mut edit.schedule
                        {
                            *minutes = if key_event.code == KeyCode::Up {
                                minutes.saturating_add(1)
                            } else {
                                minutes.saturating_sub(1).max(1)
                            };
                        }
                    }
                    return Ok(true);
                }
                KeyCode::Enter => {
                    if let Err(error) = self.save_automation_edit().await {
                        self.show_status_hint(error.to_api_error().message, Duration::from_secs(3));
                    }
                    return Ok(true);
                }
                KeyCode::Backspace => {
                    self.handle_backspace();
                    return Ok(true);
                }
                KeyCode::Left => {
                    self.cursor_index = self.cursor_index.saturating_sub(1);
                    return Ok(true);
                }
                KeyCode::Right => {
                    self.cursor_index =
                        (self.cursor_index + 1).min(self.input_buffer.chars().count());
                    return Ok(true);
                }
                KeyCode::Home => {
                    self.cursor_index = 0;
                    return Ok(true);
                }
                KeyCode::End => {
                    self.cursor_index = self.input_buffer.chars().count();
                    return Ok(true);
                }
                KeyCode::Char(c)
                    if !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.insert_char(c);
                    return Ok(true);
                }
                _ => return Ok(true),
            }
        }
        if key_event.code == KeyCode::Char('g')
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.tasks_expanded = !self.tasks_expanded;
            self.selected_queue_message = None;
            self.selected_automation =
                if self.tasks_expanded && !self.automation_targets().is_empty() {
                    Some(0)
                } else {
                    None
                };
            return Ok(true);
        }
        if key_event.code == KeyCode::Char('q')
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.selected_automation = None;
            return Ok(false);
        }
        if let Some(index) = self.selected_automation {
            match key_event.code {
                KeyCode::Esc => self.selected_automation = None,
                KeyCode::Up => self.selected_automation = Some(index.saturating_sub(1)),
                KeyCode::Down => {
                    self.selected_automation =
                        Some((index + 1).min(self.automation_targets().len().saturating_sub(1)))
                }
                KeyCode::Enter => {
                    if let Err(e) = self.selected_automation_action('s').await {
                        self.show_status_hint(e.to_api_error().message, Duration::from_secs(3));
                    }
                }
                KeyCode::Char(c @ ('e' | 'd' | 'p' | 's')) => {
                    if let Err(e) = self.selected_automation_action(c).await {
                        self.show_status_hint(e.to_api_error().message, Duration::from_secs(3));
                    }
                }
                _ => {}
            }
            return Ok(true);
        }
        Ok(false)
    }
}
