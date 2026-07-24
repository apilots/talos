use std::time::Duration;

use talos_error::TalosError;
use tokio::sync::mpsc::error::TryRecvError;

use crate::status_picker::{render_status_card, TuiStatusSnapshot};
use crate::types::CommandUpdate;
use crate::{TuiApp, SESSION_CLOSE_SLOW_NOTICE};

impl TuiApp {
    fn apply_cleared_session_snapshot(&mut self, session: &talos_sdk::SessionInfo) {
        self.apply_session_snapshot(session);
        self.input_buffer.clear();
        self.cursor_index = 0;
        self.pasted_blocks.clear();
        self.selected_pasted_block = None;
        self.pasted_image_attachments.clear();
        self.pasted_image_sequence = 0;
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
                    self.push_system_notice("Command channel dropped unexpectedly.");
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
                Ok(catalog) => match self.apply_model_catalog(catalog) {
                    Ok(()) => {
                        self.push_system_notice(format!(
                            "Available models: {}",
                            self.available_models.join(", ")
                        ));
                        self.push_right_event(format!(
                            "models loaded: {}",
                            self.available_models.len()
                        ));
                    }
                    Err(err) => {
                        self.push_system_notice(format!("Failed to load models: {}", err));
                        self.push_right_event("models load failed".to_string());
                    }
                },
                Err(err) => {
                    self.push_system_notice(format!("Failed to load models: {}", err));
                    self.push_right_event("models load failed".to_string());
                }
            },
            CommandUpdate::StatusLoaded(result) => match *result {
                Ok(status) => {
                    if let Some(info) = status.session.as_ref() {
                        self.apply_session_snapshot(info);
                    }
                    let snapshot = TuiStatusSnapshot::from_app(
                        self,
                        status.session.as_ref(),
                        &status.auth,
                        &status.workspace,
                    );
                    match snapshot {
                        Ok(snapshot) => {
                            let status_card = render_status_card(&snapshot);
                            self.render_runtime.append_system_notice(status_card);
                            self.request_chat_activity();
                            self.push_right_event("status loaded".to_string());
                        }
                        Err(err) => {
                            self.push_system_notice(format!("Failed to render status: {}", err));
                            self.push_right_event("status render failed".to_string());
                        }
                    }
                }
                Err(err) => {
                    self.push_system_notice(format!("Failed to fetch status: {}", err));
                    self.push_right_event("status load failed".to_string());
                }
            },
            CommandUpdate::SessionCompacted(result) => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.push_system_notice(
                        "Session compaction completed for the current session.".to_string(),
                    );
                    self.push_right_event("session compacted".to_string());
                }
                Err(err) => {
                    if self.in_flight {
                        self.finish_active_stream("session compaction failed", true, false);
                    }
                    self.push_system_notice(format!("Failed to compact session: {}", err));
                    self.push_right_event("session compact failed".to_string());
                }
            },
            CommandUpdate::SessionCleared(result) => match result {
                Ok(info) => {
                    self.apply_cleared_session_snapshot(&info);
                    self.push_system_notice(
                        "Session context cleared. History and token counters were reset."
                            .to_string(),
                    );
                    self.push_right_event("session cleared".to_string());
                }
                Err(err) => {
                    self.push_system_notice(format!("Failed to clear session: {}", err));
                    self.push_right_event("session clear failed".to_string());
                }
            },
            CommandUpdate::ModelSwitched { requested, result } => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.push_command_result(format!(
                        "Model changed to {}",
                        self.current_model_status_label()
                    ));
                    self.push_right_event(format!("session model updated: {}", requested));
                }
                Err(err) => {
                    self.push_system_notice(format!("Failed to switch model: {}", err));
                    self.push_right_event("model switch failed".to_string());
                }
            },
            CommandUpdate::HistoryLoaded(result) => match result {
                Ok(sessions) => {
                    let count = sessions.len();
                    if let Some(picker) = self.session_picker.as_mut() {
                        picker.set_sessions(sessions);
                    }
                    self.push_right_event(format!("history loaded: {} sessions", count));
                }
                Err(err) => {
                    self.push_right_event(format!("history load failed: {err}"));
                    if let Some(picker) = self.session_picker.as_mut() {
                        picker.set_error(err.to_string());
                    }
                }
            },
            CommandUpdate::ModelPickerSwitched { requested, result } => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.close_model_picker();
                    self.push_command_result(format!(
                        "Model changed to {}",
                        self.current_model_status_label()
                    ));
                    self.push_right_event(format!("model picker switched: {}", requested));
                }
                Err(err) => {
                    self.close_model_picker();
                    self.push_system_notice(format!("Failed to switch model: {}", err));
                    self.push_right_event("model picker switch failed".to_string());
                }
            },
            CommandUpdate::SessionHistoryLoaded { session, result } => match *result {
                Ok(load) => {
                    let sid = &session.id;
                    let model = &session.model_selection.model_ref.id;
                    let title = &session.title;
                    self.push_system_notice(format!(
                        "Session: {}  ({})\nModel: {}",
                        title, sid, model
                    ));
                    let item_count = self.replay_history_snapshot(load.history);
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
                                self.push_system_notice(format!(
                                    "Failed to recover active turn: {}",
                                    err
                                ));
                                self.push_right_event("live recovery failed".to_string());
                            }
                        }
                    }
                }
                Err(err) => {
                    let sid = &session.id;
                    self.push_system_notice(format!(
                        "Session: {}  ({})\nFailed to load session history: {}",
                        session.title, sid, err
                    ));
                    self.push_right_event("history load failed".to_string());
                }
            },
        }
    }

    pub(crate) fn poll_close_updates(&mut self) {
        let mut update = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_close_update.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    update = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    update = Some(crate::types::SessionCloseUpdate {
                        result: Err(TalosError::ClientError(
                            "session close channel dropped".to_string(),
                        )),
                        client_shutdown_completed: false,
                    });
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_close_update = None;
            self.pending_close_task = None;
        }

        let Some(update) = update else {
            return;
        };
        self.client_shutdown_completed = update.client_shutdown_completed;

        match update.result {
            Ok(session_id) => {
                self.close_in_progress = false;
                self.close_started_at = None;
                self.close_slow_notice_reported = false;
                self.closing_session_id = None;
                self.connection_status = "closed".to_string();
                self.push_right_event(format!("session close completed: {}", session_id));
                self.push_system_notice(format!(
                    "Shutting down TUI. Closed session `{}`.",
                    session_id
                ));
                self.should_exit = true;
            }
            Err(err) => {
                self.close_in_progress = false;
                self.close_started_at = None;
                self.close_slow_notice_reported = false;
                self.closing_session_id = None;
                self.push_system_notice(format!("Failed to close session: {}", err));
                self.push_right_event("session close failed".to_string());
            }
        }
    }

    pub(crate) fn poll_close_progress(&mut self) {
        if !self.close_in_progress || self.close_slow_notice_reported {
            return;
        }

        if self
            .close_started_at
            .is_some_and(|started_at| started_at.elapsed() >= SESSION_CLOSE_SLOW_NOTICE)
        {
            self.close_slow_notice_reported = true;
            self.show_status_hint(
                "Session close is still in progress. Talos will keep waiting.",
                Duration::from_secs(4),
            );
            self.push_right_event("session close still in progress".to_string());
        }
    }
}
