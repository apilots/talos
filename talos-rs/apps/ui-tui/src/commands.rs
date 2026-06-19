use std::sync::Arc;
use std::time::{Duration, Instant};

use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;
use tracing::error;

use crate::client::GatewayClient;
use crate::types::{AppState, ChatRole, CommandUpdate, QueuedMessage, SessionHistoryLoad};
use crate::utils::summarize_for_queue;
use crate::{TuiApp, MAX_QUEUE_LEN, SESSION_CLOSE_TIMEOUT};
use talos_sdk::{ProtocolSkillSuggestion, TalosSession, TimelineActiveTurnStatus};

impl TuiApp {
    pub(crate) async fn submit_input(&mut self) -> TalosResult<bool> {
        if !self.has_composer_input() {
            self.reset_input();
            return Ok(false);
        }

        let original_input = self.input_buffer.clone();
        let original_pasted_blocks = self.pasted_blocks.clone();
        let original_selected_skills = self.selected_skills.clone();
        let user_input = self.compose_user_input_text();

        let result = if self.is_command_input() {
            let command_line = self.input_buffer.trim().to_string();
            self.push_block(ChatRole::User, command_line.clone());
            self.reset_input();
            self.execute_command(&command_line).await
        } else {
            self.reset_input();
            match self
                .send_user_message_with_skills(user_input.clone(), original_selected_skills.clone())
                .await
            {
                Ok(true) => Ok(false),
                Ok(false) => {
                    self.restore_input_state(original_input, original_pasted_blocks);
                    self.selected_skills = original_selected_skills;
                    Ok(false)
                }
                Err(err) => Err(err),
            }
        };

        match result {
            Ok(should_quit) => Ok(should_quit),
            Err(err) => {
                self.push_block(ChatRole::System, format!("Failed to handle input: {}", err));
                self.push_right_event("input handling failed".to_string());
                Ok(false)
            }
        }
    }

    pub(crate) async fn execute_command(&mut self, command_line: &str) -> TalosResult<bool> {
        let trimmed = command_line.trim_start_matches('/').trim();
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or_default();
        let args = parts.collect::<Vec<_>>().join(" ");

        if self.close_in_progress && cmd != "quit" {
            self.show_status_hint(
                "Session is closing. Wait or press /quit again after timeout.",
                Duration::from_secs(2),
            );
            return Ok(false);
        }

        match cmd {
            "help" => {
                self.push_block(
                    ChatRole::System,
                    "Commands: /help /model /models /history /sessions /status /compact /clear /quit\nKeys: ↑/↓/PgUp/PgDn scroll, Ctrl+O expand tool, Ctrl+Y copy, Ctrl+U clear input"
                        .to_string(),
                );
                Ok(false)
            }
            "models" | "modes" => {
                self.refresh_models_command()?;
                Ok(false)
            }
            "history" => {
                self.open_history();
                Ok(false)
            }
            "model" => {
                let requested = args.trim();
                if requested.is_empty() {
                    self.toggle_model_picker();
                    return Ok(false);
                }

                if !self.available_models.is_empty()
                    && !self.available_models.iter().any(|m| m == requested)
                {
                    self.push_block(
                        ChatRole::System,
                        format!("Unknown model `{}`. Run /models first.", requested),
                    );
                    return Ok(false);
                }

                self.switch_model_command(requested.to_string())?;

                Ok(false)
            }
            "sessions" | "s" => {
                // Load sessions for quick switch
                if let Some(client) = self.gateway_client.clone() {
                    let _ = self.spawn_command_task("loading sessions", async move {
                        CommandUpdate::HistoryLoaded(client.list_sessions().await)
                    });
                    self.show_sessions_picker = true;
                    Ok(false)
                } else {
                    self.push_block(
                        ChatRole::System,
                        "Sessions picker requires gateway mode.".to_string(),
                    );
                    Ok(false)
                }
            }
            "status" => {
                self.status_command()?;
                Ok(false)
            }
            "compact" => {
                self.compact_session_command()?;
                Ok(false)
            }
            "clear" => {
                self.clear_session_command()?;
                Ok(false)
            }
            "quit" => {
                if self.close_in_progress {
                    if self.is_close_timed_out() {
                        self.push_right_event("session force exit".to_string());
                        self.push_block(
                            ChatRole::System,
                            "Forcing local exit while session closes in background.".to_string(),
                        );
                        self.should_exit = true;
                        return Ok(false);
                    }

                    self.show_status_hint(
                        "Session is still closing. Wait or press /quit again after timeout.",
                        Duration::from_secs(2),
                    );
                    return Ok(false);
                }
                self.close_session_command()?;
                Ok(false)
            }
            unknown => {
                self.push_block(
                    ChatRole::System,
                    format!("Unknown command `{}`. Run /help.", unknown),
                );
                Ok(false)
            }
        }
    }

    pub(crate) fn navigate_message_history(&mut self, direction: i32) {
        if self.sent_messages_history.is_empty() {
            return;
        }

        match direction {
            -1 => {
                // Up: go to previous (older) message, clamp at oldest
                let new_index = match self.message_history_index {
                    Some(0) => return, // already at oldest, do nothing
                    Some(idx) => idx - 1,
                    None => self.sent_messages_history.len() - 1,
                };
                self.message_history_index = Some(new_index);
                if let Some(msg) = self.sent_messages_history.get(new_index) {
                    self.input_buffer = msg.clone();
                    self.cursor_index = self.input_buffer.chars().count();
                }
            }
            1 => {
                // Down: go to next message in history or back to empty input
                if let Some(idx) = self.message_history_index {
                    if idx + 1 >= self.sent_messages_history.len() {
                        // At the newest item — return to empty input
                        self.input_buffer.clear();
                        self.cursor_index = 0;
                        self.message_history_index = None;
                    } else {
                        let new_index = idx + 1;
                        self.message_history_index = Some(new_index);
                        if let Some(msg) = self.sent_messages_history.get(new_index) {
                            self.input_buffer = msg.clone();
                            self.cursor_index = self.input_buffer.chars().count();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn add_message_to_history(&mut self, message: String) {
        const MAX_HISTORY: usize = 100;
        self.sent_messages_history.push_back(message);
        if self.sent_messages_history.len() > MAX_HISTORY {
            self.sent_messages_history.pop_front();
        }
        self.message_history_index = None;
    }

    #[cfg(test)]
    pub(crate) async fn send_user_message(&mut self, user_message: String) -> TalosResult<bool> {
        self.send_user_message_with_skills(user_message, Vec::new())
            .await
    }

    pub(crate) async fn send_user_message_with_skills(
        &mut self,
        user_message: String,
        skills: Vec<ProtocolSkillSuggestion>,
    ) -> TalosResult<bool> {
        if self.close_in_progress {
            self.show_status_hint(
                "Session is closing. New messages are blocked.",
                Duration::from_secs(2),
            );
            return Ok(false);
        }

        if self.in_flight || !self.message_queue.is_empty() {
            if self.message_queue.len() >= MAX_QUEUE_LEN {
                self.show_status_hint(
                    format!(
                        "Queue is full ({}/{}). Wait for current response.",
                        self.message_queue.len(),
                        MAX_QUEUE_LEN
                    ),
                    Duration::from_secs(2),
                );
                self.push_right_event("queue full".to_string());
                return Ok(false);
            }
        } else if self.message_queue.len() >= MAX_QUEUE_LEN {
            self.show_status_hint(
                format!(
                    "Queue is full ({}/{}). Wait for current response.",
                    self.message_queue.len(),
                    MAX_QUEUE_LEN
                ),
                Duration::from_secs(2),
            );
            self.push_right_event("queue full".to_string());
            return Ok(false);
        }

        self.message_queue
            .push_back(QueuedMessage::with_skills(user_message.clone(), skills));
        self.add_message_to_history(user_message.clone());
        self.push_right_event(format!(
            "queued ({}/{}): {}",
            self.message_queue.len(),
            MAX_QUEUE_LEN,
            summarize_for_queue(&user_message)
        ));
        self.dispatch_next_queued_message().await;
        Ok(true)
    }

    pub(crate) async fn dispatch_next_queued_message(&mut self) {
        if self.in_flight {
            return;
        }
        let Some(next_message) = self.message_queue.front().cloned() else {
            return;
        };

        if let Err(err) = self
            .start_stream_for_message_with_skills(
                next_message.text.clone(),
                next_message.skills.clone(),
            )
            .await
        {
            if let Some(front) = self.message_queue.front_mut() {
                front.dispatch_attempts = front.dispatch_attempts.saturating_add(1);
                if front.dispatch_attempts <= 1 {
                    self.show_status_hint(
                        "Queued message dispatch failed. Retrying once...",
                        Duration::from_secs(3),
                    );
                    self.push_right_event("queued dispatch retry".to_string());
                    return;
                }
            }

            let failed = self.message_queue.pop_front();
            self.push_block(
                ChatRole::System,
                format!("Failed to dispatch queued message: {}", err),
            );
            if let Some(failed) = failed {
                self.push_right_event(format!(
                    "queued dispatch dropped: {}",
                    summarize_for_queue(&failed.text)
                ));
            } else {
                self.push_right_event("queued dispatch failed".to_string());
            }
            return;
        }

        if self.in_flight {
            self.message_queue.pop_front();
        }
    }

    pub(crate) async fn refresh_models_from_gateway(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing app-server client".to_string(),
            ));
        };

        let models = client.list_models().await?;
        let ids: Vec<String> = models
            .iter()
            .map(|m| m.id.clone())
            .filter(|v| !v.trim().is_empty())
            .collect();
        if ids.is_empty() {
            return Err(TalosError::ClientError(
                "gateway returned empty model list".to_string(),
            ));
        }
        self.available_model_info = models;
        self.available_models = ids;
        // Only correct the model when user already has one selected that's no longer available.
        // Empty means "not yet set" — let the server determine the default via session creation.
        if !self.current_model.is_empty()
            && !self
                .available_models
                .iter()
                .any(|m| m == &self.current_model)
        {
            self.current_model = self.available_models[0].clone();
        }
        self.push_right_event(format!("models loaded: {}", self.available_models.len()));
        Ok(())
    }

    pub(crate) fn refresh_models_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing app-server client".to_string(),
            ));
        };

        self.spawn_command_task("models load started", async move {
            let result = client.list_models().await;
            CommandUpdate::ModelsLoaded(result)
        })
    }

    pub(crate) fn status_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing app-server client".to_string(),
            ));
        };
        let session_id = self.session_id.clone();
        self.spawn_command_task("status request started", async move {
            let result = if let Some(session_id) = session_id {
                client.get_session(&session_id).await.map(Some)
            } else {
                Ok(None)
            };
            CommandUpdate::StatusLoaded(result)
        })
    }

    fn active_session_id_for_control_command(&mut self, command: &str) -> Option<String> {
        if self.in_flight {
            self.show_status_hint(
                format!("Finish the active turn before running /{}.", command),
                Duration::from_secs(2),
            );
            return None;
        }

        if self.session_id.is_none() {
            self.show_status_hint(
                format!("No active session available for /{}.", command),
                Duration::from_secs(2),
            );
        }
        self.session_id.clone()
    }

    pub(crate) fn compact_session_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing app-server client".to_string(),
            ));
        };
        let Some(session_id) = self.active_session_id_for_control_command("compact") else {
            return Ok(());
        };
        self.spawn_command_task("session compact started", async move {
            CommandUpdate::SessionCompacted(client.compact_session(&session_id).await)
        })
    }

    pub(crate) fn clear_session_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing app-server client".to_string(),
            ));
        };
        let Some(session_id) = self.active_session_id_for_control_command("clear") else {
            return Ok(());
        };
        self.spawn_command_task("session clear started", async move {
            CommandUpdate::SessionCleared(client.clear_session(&session_id).await)
        })
    }

    pub(crate) fn switch_model_command(&mut self, requested: String) -> TalosResult<()> {
        if self.session_id.is_none() {
            self.current_model = requested.clone();
            self.push_right_event(format!("draft model set: {}", requested));
            return Ok(());
        }
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing app-server client".to_string(),
            ));
        };
        let session_id = self.session_id.clone().expect("session checked above");
        self.spawn_command_task(format!("model switch started: {}", requested), async move {
            let result = client.update_session_model(&session_id, &requested).await;
            CommandUpdate::ModelSwitched { requested, result }
        })
    }

    pub(crate) fn open_history(&mut self) {
        let Some(client) = self.gateway_client.clone() else {
            self.push_block(ChatRole::System, "No gateway client connected.".to_string());
            return;
        };
        self.current_state = AppState::History;
        let _ = self.spawn_command_task("loading history", async move {
            CommandUpdate::HistoryLoaded(client.list_sessions().await)
        });
    }

    pub(crate) fn toggle_model_picker(&mut self) {
        if self.show_model_picker {
            self.show_model_picker = false;
            return;
        }
        if self.available_models.is_empty() {
            self.push_block(
                ChatRole::System,
                "No models loaded. Run /models first.".to_string(),
            );
            return;
        }
        // Pre-select current model
        self.model_picker_selected = self
            .available_models
            .iter()
            .position(|m| m == &self.current_model)
            .unwrap_or(0);
        self.show_model_picker = true;
    }

    pub(crate) fn switch_to_history_session(&mut self) {
        let Some(session) = self.history_sessions.get(self.history_selected).cloned() else {
            return;
        };
        self.current_state = AppState::Chat;
        self.load_session_history(session);
    }

    pub(crate) fn switch_to_sessions_picker(&mut self) {
        let Some(session) = self
            .history_sessions
            .get(self.sessions_picker_selected)
            .cloned()
        else {
            return;
        };
        self.show_sessions_picker = false;
        self.load_session_history(session);
    }

    pub(crate) fn load_session_history(&mut self, session: TalosSession) {
        let session_id = session.context.session_id.clone();
        let model = session.config.llm.model.clone();
        self.session_id = Some(session_id.clone());
        self.current_model = model;
        self.session_usage = session.state.usage.clone();
        self.turn_metrics = None;
        self.turn_metrics_live = false;
        // Reset chat and input history for the new session
        self.chat_blocks.clear();
        self.rendered_chat_lines.clear();
        self.sent_messages_history.clear();
        self.message_history_index = None;
        self.push_right_event(format!("switched to session: {}", session_id));

        // Abort any in-flight command — session switch takes priority and the
        // previous result is irrelevant now.
        if let Some(task) = self.pending_command_task.take() {
            task.abort();
        }
        self.pending_command_update = None;

        let Some(client) = self.gateway_client.clone() else {
            self.push_block(ChatRole::System, format!("Session: {}", session_id));
            return;
        };
        let _ = self.spawn_command_task("loading session history", async move {
            let result = match client.get_session_history(&session_id).await {
                Ok(history) => {
                    let live_recovery = match client.get_active_turn(&session_id).await {
                        Ok(active_turn)
                            if matches!(
                                active_turn.status,
                                TimelineActiveTurnStatus::Running
                                    | TimelineActiveTurnStatus::WaitingUserAction
                                    | TimelineActiveTurnStatus::Cancelling
                            ) =>
                        {
                            Some(
                                client
                                    .recover_session(
                                        &session_id,
                                        history.last_stable_event_id.clone(),
                                    )
                                    .await,
                            )
                        }
                        Ok(_) => None,
                        Err(err) => Some(Err(err)),
                    };
                    Ok(SessionHistoryLoad {
                        history,
                        live_recovery,
                    })
                }
                Err(err) => Err(err),
            };
            CommandUpdate::SessionHistoryLoaded { result, session }
        });
    }

    pub(crate) fn delete_history_session(&mut self) {
        let Some(session) = self.history_sessions.get(self.history_selected).cloned() else {
            return;
        };
        let session_id = session.context.session_id.clone();
        let Some(client) = self.gateway_client.clone() else {
            return;
        };
        self.history_sessions.remove(self.history_selected);
        if self.history_selected > 0 && self.history_selected >= self.history_sessions.len() {
            self.history_selected = self.history_sessions.len().saturating_sub(1);
        }
        let sid_log = session_id.clone();
        tokio::spawn(async move {
            let _ = client.delete_session(&session_id).await;
        });
        self.push_right_event(format!("deleted session: {}", sid_log));
    }

    pub(crate) fn confirm_model_picker(&mut self) {
        let Some(model) = self
            .available_models
            .get(self.model_picker_selected)
            .cloned()
        else {
            return;
        };
        if self.session_id.is_none() {
            self.current_model = model.clone();
            self.show_model_picker = false;
            self.push_right_event(format!("draft model set: {}", model));
            return;
        }
        let Some(client) = self.gateway_client.clone() else {
            self.show_model_picker = false;
            return;
        };
        let session_id = self.session_id.clone().expect("session checked above");
        let requested = model.clone();
        let _ = self.spawn_command_task(
            format!("model picker switching to: {}", model),
            async move {
                let result = client.update_session_model(&session_id, &requested).await;
                CommandUpdate::ModelPickerSwitched { requested, result }
            },
        );
    }

    pub(crate) fn close_session_command(&mut self) -> TalosResult<()> {
        if self.close_in_progress {
            return Ok(());
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing app-server client".to_string(),
            ));
        };
        let session_id = self.session_id.clone();
        let closing_session_id = session_id.clone().unwrap_or_else(|| "none".to_string());
        let (tx, rx) = mpsc::channel(1);
        self.pending_close_update = Some(rx);
        self.close_in_progress = true;
        self.close_started_at = Some(Instant::now());
        self.close_timeout_reported = false;
        self.closing_session_id = Some(closing_session_id.clone());
        self.connection_status = "closing".to_string();
        self.show_status_hint(
            "Closing session. Memory flush continues in background.",
            Duration::from_secs(2),
        );
        self.push_right_event(format!("session close started: {}", closing_session_id));

        let handle = tokio::spawn(async move {
            let result =
                close_session_and_shutdown(client, session_id, closing_session_id.clone()).await;
            if tx.send(result).await.is_err() {
                error!(
                    session_id = %closing_session_id,
                    "failed to deliver close session result to TUI task"
                );
            }
        });
        self.pending_close_task = Some(handle);
        Ok(())
    }

    pub(crate) fn spawn_command_task<F>(
        &mut self,
        event: impl Into<String>,
        fut: F,
    ) -> TalosResult<()>
    where
        F: std::future::Future<Output = CommandUpdate> + Send + 'static,
    {
        if self.pending_command_update.is_some() {
            self.show_status_hint("Another command is still running.", Duration::from_secs(2));
            return Ok(());
        }

        let (tx, rx) = mpsc::channel(1);
        self.pending_command_update = Some(rx);
        self.push_right_event(event.into());
        let handle = tokio::spawn(async move {
            let result = fut.await;
            if tx.send(result).await.is_err() {
                error!("failed to deliver command result to TUI task");
            }
        });
        self.pending_command_task = Some(handle);
        Ok(())
    }

    pub(crate) fn is_close_timed_out(&self) -> bool {
        self.close_started_at
            .map(|started_at| started_at.elapsed() >= SESSION_CLOSE_TIMEOUT)
            .unwrap_or(false)
    }
}

async fn close_session_and_shutdown(
    client: Arc<dyn GatewayClient>,
    session_id: Option<String>,
    fallback_session_id: String,
) -> TalosResult<String> {
    let close_result = if let Some(session_id) = session_id.as_deref() {
        client
            .close_session(session_id)
            .await
            .map(|_| session_id.to_string())
    } else {
        Ok(fallback_session_id)
    };
    let shutdown_result = client.shutdown().await;
    match (close_result, shutdown_result) {
        (Ok(session_id), Ok(())) => Ok(session_id),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(close_error), Err(shutdown_error)) => Err(TalosError::ClientError(format!(
            "close session failed: {close_error}; shutdown failed: {shutdown_error}"
        ))),
    }
}
