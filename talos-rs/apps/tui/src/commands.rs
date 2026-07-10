use std::sync::Arc;
use std::time::{Duration, Instant};

use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;
use tracing::error;

use crate::client::GatewayClient;
use crate::input_history::InputHistoryNavigation;
use crate::model_picker::{ModelPickerSelection, ModelPickerState};
use crate::session_picker::SessionPickerState;
use crate::types::{CommandUpdate, QueuedMessage, SessionHistoryLoad, StatusLoad};
use crate::utils::summarize_for_queue;
use crate::{TuiApp, MAX_QUEUE_LEN, SESSION_CLOSE_TIMEOUT};
use talos_sdk::{SessionInfo, SkillSuggestion, TimelineActiveTurnStatus};

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
            if command_should_enter_transcript(&command_line) {
                self.render_runtime
                    .append_command_input(command_line.clone());
            }
            self.request_chat_activity();
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
                self.push_system_notice(format!("Failed to handle input: {}", err));
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
                self.push_system_notice(
                    "Commands: /help /model /models /sessions /status /compact /clear /quit\nKeys: ↑/↓ history, Home/End cursor, Ctrl+O expand tool, Ctrl+U clear input"
                        .to_string(),
                );
                Ok(false)
            }
            "models" | "modes" => {
                self.refresh_models_command()?;
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
                    self.push_system_notice(format!(
                        "Unknown model `{}`. Run /models first.",
                        requested
                    ));
                    return Ok(false);
                }

                self.switch_model_command(requested.to_string())?;

                Ok(false)
            }
            "sessions" | "s" => {
                if let Some(client) = self.gateway_client.clone() {
                    self.session_picker = Some(SessionPickerState::loading());
                    let _ = self.spawn_command_task("loading sessions", async move {
                        CommandUpdate::HistoryLoaded(client.list_sessions().await)
                    });
                    Ok(false)
                } else {
                    self.push_system_notice("Sessions picker requires gateway mode.".to_string());
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
                        self.push_system_notice(
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
                self.push_system_notice(format!("Unknown command `{}`. Run /help.", unknown));
                Ok(false)
            }
        }
    }

    pub(crate) fn navigate_message_history(&mut self, direction: i32) {
        match direction {
            -1 => {
                if let Some(message) = self.input_history.previous() {
                    self.input_buffer = message.to_string();
                    self.cursor_index = self.input_buffer.chars().count();
                }
            }
            1 => match self.input_history.next() {
                InputHistoryNavigation::Selected(message) => {
                    self.input_buffer = message.to_string();
                    self.cursor_index = self.input_buffer.chars().count();
                }
                InputHistoryNavigation::Cleared => {
                    self.input_buffer.clear();
                    self.cursor_index = 0;
                }
                InputHistoryNavigation::Unchanged => {}
            },
            _ => {}
        }
    }

    pub(crate) fn add_message_to_history(&mut self, message: String) {
        self.input_history.push(message);
    }

    pub(crate) async fn send_user_message_with_skills(
        &mut self,
        user_message: String,
        skills: Vec<SkillSuggestion>,
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
            self.push_system_notice(format!("Failed to dispatch queued message: {}", err));
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
                "missing gateway client".to_string(),
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
        if self.current_model.is_empty()
            || !self
                .available_models
                .iter()
                .any(|m| m == &self.current_model)
        {
            self.current_model = self.available_models[0].clone();
        }
        self.sync_current_reasoning_effort_from_model();
        self.push_right_event(format!("models loaded: {}", self.available_models.len()));
        Ok(())
    }

    pub(crate) fn refresh_models_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client".to_string(),
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
                "missing gateway client".to_string(),
            ));
        };
        let session_id = self.session_id.clone();
        self.spawn_command_task("status request started", async move {
            let result = async {
                let session = if let Some(session_id) = session_id {
                    Some(client.get_session(&session_id).await?)
                } else {
                    None
                };
                let auth = client.read_auth_session().await?;
                let workspace = client.workspace_info().await?;
                Ok(StatusLoad {
                    session,
                    auth,
                    workspace,
                })
            }
            .await;
            CommandUpdate::StatusLoaded(Box::new(result))
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
                "missing gateway client".to_string(),
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
                "missing gateway client".to_string(),
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
        let Some(model_ref) = self.model_ref_for_id(&requested) else {
            return Err(TalosError::ClientError(format!(
                "model '{}' is not available in the current model catalog",
                requested
            )));
        };
        if self.session_id.is_none() {
            self.current_model = requested.clone();
            self.sync_current_reasoning_effort_from_model();
            self.push_command_result(format!(
                "Model changed to {}",
                self.current_model_status_label()
            ));
            self.push_right_event(format!("draft model set: {}", requested));
            return Ok(());
        }
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client".to_string(),
            ));
        };
        let session_id = self.session_id.clone().expect("session checked above");
        self.spawn_command_task(format!("model switch started: {}", requested), async move {
            let result = client.update_session_model(&session_id, &model_ref).await;
            CommandUpdate::ModelSwitched {
                requested,
                reasoning_effort: None,
                result,
            }
        })
    }

    fn model_ref_for_id(&self, model_id: &str) -> Option<talos_sdk::ModelRef> {
        self.available_model_info
            .iter()
            .find(|model| model.id == model_id)
            .map(|model| talos_sdk::ModelRef {
                provider: model.provider.clone(),
                id: model.id.clone(),
            })
    }

    pub(crate) fn toggle_model_picker(&mut self) {
        if self.model_picker.is_some() {
            self.close_model_picker();
            return;
        }
        if self.available_models.is_empty() {
            self.push_system_notice("No models loaded. Run /models first.");
            return;
        }
        self.model_picker = Some(ModelPickerState::new(
            &self.available_model_info,
            &self.current_model,
            self.current_reasoning_effort,
        ));
    }

    pub(crate) fn close_model_picker(&mut self) {
        self.model_picker = None;
    }

    pub(crate) fn navigate_model_picker(&mut self, direction: i32) {
        if let Some(picker) = self.model_picker.as_mut() {
            picker.move_selection(direction);
        }
    }

    pub(crate) fn back_model_picker(&mut self) {
        let should_close = self
            .model_picker
            .as_mut()
            .map(ModelPickerState::back)
            .unwrap_or(false);
        if should_close {
            self.close_model_picker();
        }
    }

    pub(crate) fn load_session_history(&mut self, session: SessionInfo) {
        let session_id = session.id.clone();
        let model = session.model_ref.id.clone();
        self.session_id = Some(session_id.clone());
        self.current_model = model;
        self.sync_current_reasoning_effort_from_model();
        self.session_usage = session.usage.clone();
        self.turn_metrics = None;
        self.turn_metrics_live = false;
        // Reset chat and input history for the new session
        self.render_runtime.reset_transcript();
        self.rendered_chat_lines.clear();
        self.rendered_chat_cache_key = None;
        self.input_history.clear();
        self.push_right_event(format!("switched to session: {}", session_id));

        // Abort any in-flight command — session switch takes priority and the
        // previous result is irrelevant now.
        if let Some(task) = self.pending_command_task.take() {
            task.abort();
        }
        self.pending_command_update = None;

        let Some(client) = self.gateway_client.clone() else {
            self.push_system_notice(format!("Session: {}", session_id));
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
                                    .recover_timeline(
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
            CommandUpdate::SessionHistoryLoaded {
                result: Box::new(result),
                session,
            }
        });
    }

    pub(crate) fn confirm_model_picker(&mut self) {
        let Some(selection) = self
            .model_picker
            .as_mut()
            .and_then(ModelPickerState::confirm)
        else {
            return;
        };
        self.apply_model_picker_selection(selection);
    }

    fn apply_model_picker_selection(&mut self, selection: ModelPickerSelection) {
        let model = selection.model;
        let reasoning_effort = selection.reasoning_effort;
        if self.session_id.is_none() {
            self.current_model = model.clone();
            self.set_current_reasoning_effort(reasoning_effort);
            self.close_model_picker();
            self.push_command_result(format!(
                "Model changed to {}",
                self.current_model_status_label()
            ));
            self.push_right_event(format!("draft model set: {}", model));
            return;
        }
        let Some(client) = self.gateway_client.clone() else {
            self.close_model_picker();
            return;
        };
        let session_id = self.session_id.clone().expect("session checked above");
        let requested = model.clone();
        let Some(model_ref) = self.model_ref_for_id(&requested) else {
            self.close_model_picker();
            self.push_system_notice(format!(
                "Model `{}` is not available in the current catalog.",
                requested
            ));
            return;
        };
        let _ = self.spawn_command_task(
            format!("model picker switching to: {}", model),
            async move {
                let result = client.update_session_model(&session_id, &model_ref).await;
                CommandUpdate::ModelPickerSwitched {
                    requested,
                    reasoning_effort,
                    result,
                }
            },
        );
    }

    pub(crate) fn close_session_command(&mut self) -> TalosResult<()> {
        if self.close_in_progress {
            return Ok(());
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client".to_string(),
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

fn command_should_enter_transcript(command_line: &str) -> bool {
    let command = command_line
        .trim_start()
        .trim_start_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or_default();
    !matches!(command, "sessions" | "s")
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
