use std::time::Duration;

use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosUsage;
use tokio::sync::mpsc::error::TryRecvError;

use crate::types::{AppState, ChatRole, CommandUpdate};
use crate::TuiApp;

impl TuiApp {
    fn append_usage_lines(lines: &mut Vec<String>, label: &str, usage: &TalosUsage) {
        lines.push(format!("{label}.usage.input: {}", usage.input_tokens));
        lines.push(format!("{label}.usage.output: {}", usage.output_tokens));
        lines.push(format!("{label}.usage.total: {}", usage.total_tokens));
        lines.push(format!(
            "{label}.usage.cache_create: {}",
            usage
                .cache_creation_input_tokens
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ));
        lines.push(format!(
            "{label}.usage.cache_read: {}",
            usage
                .cache_read_input_tokens
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ));
    }

    fn apply_cleared_session_snapshot(&mut self, session: &talos_sdk::SessionInfo) {
        self.apply_session_snapshot(session);
        self.input_buffer.clear();
        self.cursor_index = 0;
        self.pasted_blocks.clear();
        self.selected_pasted_block = None;
        self.reset_chat_surface();
    }

    pub(crate) fn poll_command_updates(&mut self) {
        let mut update: Option<CommandUpdate> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_command_update.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    update = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.push_block(
                        ChatRole::System,
                        "Command channel dropped unexpectedly.".to_string(),
                    );
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_command_update = None;
            self.pending_command_task = None;
        }

        let Some(update) = update else {
            return;
        };

        match update {
            CommandUpdate::ModelsLoaded(result) => match result {
                Ok(models) => {
                    self.available_model_info = models;
                    self.available_models = self
                        .available_model_info
                        .iter()
                        .map(|model| model.id.clone())
                        .filter(|id| !id.trim().is_empty())
                        .collect();
                    if !self
                        .available_models
                        .iter()
                        .any(|model| model == &self.current_model)
                    {
                        if let Some(first) = self.available_models.first() {
                            self.current_model = first.clone();
                        }
                    }
                    self.push_block(
                        ChatRole::System,
                        format!("Available models: {}", self.available_models.join(", ")),
                    );
                    self.push_right_event(format!(
                        "models loaded: {}",
                        self.available_models.len()
                    ));
                }
                Err(err) => {
                    self.push_block(ChatRole::System, format!("Failed to load models: {}", err));
                    self.push_right_event("models load failed".to_string());
                }
            },
            CommandUpdate::StatusLoaded(result) => match result {
                Ok(session_info) => {
                    let working_directory = session_info
                        .as_ref()
                        .and_then(|session| session.current_directory.as_ref())
                        .cloned()
                        .or_else(|| self.current_project_root.clone());
                    let mut lines = vec![
                        "mode: app-server".to_string(),
                        format!("model: {}", self.current_model),
                        format!(
                            "working_directory: {}",
                            working_directory
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|| "(server-managed)".to_string())
                        ),
                        format!(
                            "session: {}",
                            self.session_id
                                .clone()
                                .unwrap_or_else(|| "none".to_string())
                        ),
                        format!("connection: {}", self.connection_status),
                        format!("in_flight: {}", self.in_flight),
                        format!("events: {}", self.trace_event_count),
                    ];
                    Self::append_usage_lines(&mut lines, "local", &self.displayed_usage());
                    if let Some(info) = session_info {
                        self.apply_session_snapshot(&info);
                        lines.push(format!("server.model: {}", info.model));
                        lines.push(format!("server.messages: {}", info.message_count));
                        Self::append_usage_lines(&mut lines, "server", &info.usage);
                    }
                    self.push_block(ChatRole::System, lines.join("\n"));
                    self.push_right_event("status loaded".to_string());
                }
                Err(err) => {
                    self.push_block(ChatRole::System, format!("Failed to fetch status: {}", err));
                    self.push_right_event("status load failed".to_string());
                }
            },
            CommandUpdate::SessionCompacted(result) => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.push_block(
                        ChatRole::System,
                        "Session compaction completed for the current session.".to_string(),
                    );
                    self.push_right_event("session compacted".to_string());
                }
                Err(err) => {
                    self.push_block(
                        ChatRole::System,
                        format!("Failed to compact session: {}", err),
                    );
                    self.push_right_event("session compact failed".to_string());
                }
            },
            CommandUpdate::SessionCleared(result) => match result {
                Ok(info) => {
                    self.apply_cleared_session_snapshot(&info);
                    self.push_block(
                        ChatRole::System,
                        "Session context cleared. History and token counters were reset."
                            .to_string(),
                    );
                    self.push_right_event("session cleared".to_string());
                }
                Err(err) => {
                    self.push_block(
                        ChatRole::System,
                        format!("Failed to clear session: {}", err),
                    );
                    self.push_right_event("session clear failed".to_string());
                }
            },
            CommandUpdate::ModelSwitched { requested, result } => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.push_block(
                        ChatRole::System,
                        format!("Model switched to `{}`.", self.current_model),
                    );
                    self.push_right_event(format!("session model updated: {}", requested));
                }
                Err(err) => {
                    self.push_block(ChatRole::System, format!("Failed to switch model: {}", err));
                    self.push_right_event("model switch failed".to_string());
                }
            },
            CommandUpdate::HistoryLoaded(result) => match result {
                Ok(sessions) => {
                    let mut sessions = sessions;
                    sessions.sort_by(|left, right| {
                        right
                            .last_activity
                            .cmp(&left.last_activity)
                            .then(right.created_at.cmp(&left.created_at))
                    });
                    self.history_sessions = sessions;
                    self.history_selected = 0;
                    self.push_right_event(format!(
                        "history loaded: {} sessions",
                        self.history_sessions.len()
                    ));
                }
                Err(err) => {
                    self.push_right_event(format!("history load failed: {err}"));
                    self.current_state = AppState::Chat;
                }
            },
            CommandUpdate::ModelPickerSwitched { requested, result } => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.show_model_picker = false;
                    self.push_block(
                        ChatRole::System,
                        format!("Model switched to `{}`.", self.current_model),
                    );
                    self.push_right_event(format!("model picker switched: {}", requested));
                }
                Err(err) => {
                    self.show_model_picker = false;
                    self.push_block(ChatRole::System, format!("Failed to switch model: {}", err));
                    self.push_right_event("model picker switch failed".to_string());
                }
            },
            CommandUpdate::SessionHistoryLoaded { session, result } => match result {
                Ok(load) => {
                    let sid = &session.id;
                    let model = &session.model;
                    let title = &session.title;
                    self.push_block(
                        ChatRole::System,
                        format!("Session: {}  ({})\nModel: {}", title, sid, model),
                    );
                    let item_count = self.replay_history_snapshot(load.history);
                    self.message_history_index = None;
                    self.push_right_event(format!("history loaded: {} items", item_count));
                    if let Some(live_recovery) = load.live_recovery {
                        match live_recovery {
                            Ok(snapshot) => {
                                let event_count = self.replay_recovery_snapshot(snapshot);
                                self.push_right_event(format!(
                                    "live recovery replayed: {} events",
                                    event_count
                                ));
                            }
                            Err(err) => {
                                self.push_block(
                                    ChatRole::System,
                                    format!("Failed to recover active turn: {}", err),
                                );
                                self.push_right_event("live recovery failed".to_string());
                            }
                        }
                    }
                }
                Err(err) => {
                    let sid = &session.id;
                    self.push_block(
                        ChatRole::System,
                        format!(
                            "Session: {}  ({})\nFailed to load session history: {}",
                            session.title, sid, err
                        ),
                    );
                    self.push_right_event("history load failed".to_string());
                }
            },
        }
    }

    pub(crate) fn poll_close_updates(&mut self) {
        let mut result: Option<TalosResult<String>> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_close_update.as_mut() {
            match rx.try_recv() {
                Ok(update) => {
                    result = Some(update);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    result = Some(Err(TalosError::ClientError(
                        "session close channel dropped".to_string(),
                    )));
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_close_update = None;
            self.pending_close_task = None;
        }

        match result {
            Some(Ok(session_id)) => {
                self.close_in_progress = false;
                self.close_started_at = None;
                self.close_timeout_reported = false;
                self.closing_session_id = None;
                self.connection_status = "closed".to_string();
                self.push_right_event(format!("session close completed: {}", session_id));
                self.push_block(
                    ChatRole::System,
                    format!("Shutting down TUI. Closed session `{}`.", session_id),
                );
                self.should_exit = true;
            }
            Some(Err(err)) => {
                self.close_in_progress = false;
                self.close_started_at = None;
                self.close_timeout_reported = false;
                self.closing_session_id = None;
                self.push_block(
                    ChatRole::System,
                    format!("Failed to close session: {}", err),
                );
                self.push_right_event("session close failed".to_string());
            }
            None => {}
        }
    }

    pub(crate) fn poll_close_timeout(&mut self) {
        if !self.close_in_progress || self.close_timeout_reported {
            return;
        }

        if self.is_close_timed_out() {
            self.close_timeout_reported = true;
            self.show_status_hint(
                "Session close timed out. Press /quit again to force exit.",
                Duration::from_secs(4),
            );
            self.push_right_event("session close timed out".to_string());
        }
    }
}
