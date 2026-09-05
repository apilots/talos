use std::sync::Arc;
use std::time::{Duration, Instant};

use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;
use tracing::error;

use crate::client::GatewayClient;
use crate::input_history::InputHistoryNavigation;
use crate::model_picker::{ModelPickerSelection, ModelPickerState};
use crate::permission_picker::PermissionPickerState;
use crate::session_picker::SessionPickerState;
use crate::types::{CommandUpdate, SessionCloseUpdate, SessionHistoryLoad, StatusLoad};
use crate::{TuiApp, SESSION_CLOSE_FORCE_TIMEOUT};
use talos_sdk::{SessionInfo, SkillSuggestion};

mod permission;

impl TuiApp {
    pub(crate) async fn submit_input(&mut self) -> TalosResult<bool> {
        if !self.has_composer_input() && self.editing_queue_message_id.is_none() {
            self.reset_input();
            return Ok(false);
        }
        if self.reject_input_while_context_compacting() {
            return Ok(false);
        }

        let result = if self.editing_queue_message_id.is_some() {
            let user_input = self.compose_user_input_text()?;
            if self.update_queued_message_text(user_input).await {
                self.reset_input();
            }
            Ok(false)
        } else if self.is_command_input() {
            let command_line = self.input_buffer.trim().to_string();
            if command_should_enter_transcript(&command_line) {
                self.render_runtime
                    .append_command_input(command_line.clone());
            }
            self.request_chat_activity();
            self.reset_input();
            self.execute_command(&command_line).await
        } else {
            let user_input = match self.compose_user_input_text() {
                Ok(user_input) => user_input,
                Err(error) => {
                    self.push_system_notice(format!("Invalid image attachment marker: {error}"));
                    self.push_right_event("image attachment marker is invalid".to_string());
                    return Ok(false);
                }
            };
            let file_ids = match self.upload_pending_image_attachments().await {
                Ok(file_ids) => file_ids,
                Err(error) => {
                    self.push_system_notice(format!("Failed to upload image attachment: {error}"));
                    self.push_right_event("image attachment upload failed".to_string());
                    return Ok(false);
                }
            };
            let original_input = self.input_buffer.clone();
            let original_pasted_blocks = self.pasted_blocks.clone();
            let original_pasted_images = self.pasted_image_attachments.clone();
            let original_pasted_image_sequence = self.pasted_image_sequence;
            let original_selected_skills = self.selected_skills.clone();
            self.reset_input();
            match self
                .send_user_message_with_skills(
                    user_input.clone(),
                    original_selected_skills.clone(),
                    file_ids,
                )
                .await
            {
                Ok(true) => Ok(false),
                Ok(false) => {
                    self.input_buffer = original_input;
                    self.cursor_index = self.input_buffer.chars().count();
                    self.pasted_blocks = original_pasted_blocks;
                    self.selected_pasted_block = if self.pasted_blocks.is_empty() {
                        None
                    } else {
                        Some(self.pasted_blocks.len().saturating_sub(1))
                    };
                    self.pasted_image_attachments = original_pasted_images;
                    self.pasted_image_sequence = original_pasted_image_sequence;
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
        if self.reject_input_while_context_compacting() {
            return Ok(false);
        }
        let trimmed = command_line.trim_start_matches('/').trim();
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or_default();
        let args = parts.collect::<Vec<_>>().join(" ");

        if self.close_in_progress && cmd != "quit" {
            self.show_status_hint("Session is closing.", Duration::from_secs(2));
            return Ok(false);
        }

        match cmd {
            "help" => {
                self.push_system_notice(
                    "Commands: /help /model /models /permissions /sessions /status /compact /clear /quit\nKeys: ↑/↓ history, Home/End cursor, Ctrl+V paste image, Ctrl+O expand tool, Ctrl+U clear input"
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
            "permissions" => {
                if self.in_flight {
                    self.push_system_notice(
                        "Permissions can only be changed while the session is idle.".to_string(),
                    );
                } else if self.session_id.is_none() {
                    self.push_system_notice("Permissions require an active session.".to_string());
                } else {
                    self.permission_picker =
                        Some(PermissionPickerState::new(self.current_permission_mode));
                }
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
                    if self.can_force_close() {
                        if let Some(task) = self.pending_close_task.take() {
                            task.abort();
                        }
                        self.pending_close_update = None;
                        self.push_right_event("session force exit".to_string());
                        self.push_system_notice(
                            "Forcing local exit before session close completes.".to_string(),
                        );
                        self.force_exit_without_shutdown = true;
                        self.should_exit = true;
                        return Ok(false);
                    }

                    self.show_status_hint("Session is still closing.", Duration::from_secs(2));
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
        file_ids: Vec<String>,
    ) -> TalosResult<bool> {
        if self.reject_input_while_context_compacting() {
            return Ok(false);
        }
        if self.close_in_progress {
            self.show_status_hint(
                "Session is closing. New messages are blocked.",
                Duration::from_secs(2),
            );
            return Ok(false);
        }

        if self.in_flight || !self.message_queue.messages.is_empty() {
            let accepted = self
                .enqueue_message(user_message.clone(), skills, file_ids)
                .await;
            if accepted && !user_message.is_empty() {
                self.add_message_to_history(user_message);
            }
            return Ok(accepted);
        }

        self.start_stream_for_message_with_skills(user_message.clone(), skills, file_ids)
            .await?;
        if !user_message.is_empty() {
            self.add_message_to_history(user_message);
        }
        Ok(true)
    }

    pub(crate) async fn refresh_models_from_gateway(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::internal("missing gateway client".to_string()));
        };

        let catalog = client.list_models().await?;
        self.apply_model_catalog(catalog)?;
        self.push_right_event(format!("models loaded: {}", self.available_models.len()));
        Ok(())
    }

    pub(crate) fn apply_model_catalog(
        &mut self,
        catalog: talos_sdk::ModelListResponse,
    ) -> TalosResult<()> {
        let talos_sdk::ModelListResponse {
            models,
            default_model_selection,
        } = catalog;
        let model_keys = models
            .iter()
            .map(crate::model_selection::model_key)
            .filter(|key| !key.trim().is_empty())
            .collect::<Vec<_>>();
        if model_keys.is_empty() {
            return Err(TalosError::internal(
                "gateway returned empty model list".to_string(),
            ));
        }
        models
            .iter()
            .find(|model| {
                model.provider == default_model_selection.model_ref.provider
                    && model.model.id == default_model_selection.model_ref.id
            })
            .ok_or_else(|| {
                TalosError::internal(format!(
                    "configured default model '{}:{}' is missing from the gateway model catalog",
                    default_model_selection.model_ref.provider,
                    default_model_selection.model_ref.id
                ))
            })?;
        let current_model_retained = models.iter().any(|model| {
            model.provider == self.current_model_provider && model.model.id == self.current_model
        });
        let reasoning_effort_override = self.reasoning_effort_override;
        self.available_model_info = models;
        self.available_models = model_keys;
        if current_model_retained {
            self.set_reasoning_effort_override(reasoning_effort_override);
        } else {
            self.set_current_model_selection(&default_model_selection);
        }
        Ok(())
    }

    pub(crate) fn refresh_models_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::internal("missing gateway client".to_string()));
        };

        self.spawn_command_task("models load started", async move {
            let result = client.list_models().await;
            CommandUpdate::ModelsLoaded(result)
        })
    }

    pub(crate) fn status_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::internal("missing gateway client".to_string()));
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
                Ok(StatusLoad { session, auth })
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
            return Err(TalosError::internal("missing gateway client".to_string()));
        };
        let Some(session_id) = self.active_session_id_for_control_command("compact") else {
            return Ok(());
        };
        if self.pending_command_update.is_some() {
            self.show_status_hint("Another command is still running.", Duration::from_secs(2));
            return Ok(());
        }

        self.abort_pending_stream_task();
        self.in_flight = true;
        self.received_output_delta = false;
        self.stream_started_at = Some(Instant::now());
        self.last_stream_event_at = Some(Instant::now());
        self.pending_stream = None;
        self.active_stream_turn_id = None;
        self.last_stream_sequence = None;
        self.last_stream_event_id = None;
        self.context_compaction_stage = None;

        let (stream_tx, stream_rx) = mpsc::channel(1);
        self.pending_stream_connect = Some(stream_rx);
        let (command_tx, command_rx) = mpsc::channel(1);
        self.pending_command_update = Some(command_rx);
        self.push_right_event("session compact started".to_string());

        let handle = tokio::spawn(async move {
            let stream = async {
                let recovery = client.recover_timeline(&session_id, None).await?;
                let since_event_id = recovery.replay.high_water_mark_event_id;
                client
                    .subscribe_session_timeline(&session_id, since_event_id)
                    .await
            }
            .await;
            match stream {
                Ok(receiver) => {
                    if stream_tx.send(Ok(receiver)).await.is_err() {
                        error!("failed to deliver compaction stream to TUI task");
                        return;
                    }
                }
                Err(err) => {
                    let message = err.to_string();
                    let _ = stream_tx.send(Err(err)).await;
                    let _ = command_tx
                        .send(CommandUpdate::SessionCompacted(Err(TalosError::internal(
                            message,
                        ))))
                        .await;
                    return;
                }
            }

            let update = CommandUpdate::SessionCompacted(client.compact_session(&session_id).await);
            if command_tx.send(update).await.is_err() {
                error!("failed to deliver command result to TUI task");
            }
        });
        self.pending_command_task = Some(handle);
        Ok(())
    }

    pub(crate) fn clear_session_command(&mut self) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::internal("missing gateway client".to_string()));
        };
        let Some(session_id) = self.active_session_id_for_control_command("clear") else {
            return Ok(());
        };
        self.spawn_command_task("session clear started", async move {
            CommandUpdate::SessionCleared(client.clear_session(&session_id).await)
        })
    }

    pub(crate) fn switch_model_command(&mut self, requested: String) -> TalosResult<()> {
        let Some(model_selection) =
            crate::model_selection::selection_for_key(&self.available_model_info, &requested, None)
        else {
            return Err(TalosError::internal(format!(
                "model '{}' is not available in the current model catalog",
                requested
            )));
        };
        if self.session_id.is_none() {
            self.set_current_model_selection(&model_selection);
            self.push_command_result(format!(
                "Model changed to {}",
                self.current_model_status_label()
            ));
            self.push_right_event(format!("draft model set: {}", requested));
            return Ok(());
        }
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::internal("missing gateway client".to_string()));
        };
        let session_id = self.session_id.clone().expect("session checked above");
        self.spawn_command_task(format!("model switch started: {}", requested), async move {
            let result = client
                .update_session_model_selection(&session_id, &model_selection)
                .await;
            CommandUpdate::ModelSwitched { requested, result }
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
        let picker = ModelPickerState::new(
            &self.available_model_info,
            &talos_sdk::ModelRef {
                provider: self.current_model_provider.clone(),
                id: self.current_model.clone(),
            },
            self.reasoning_effort_override,
        );
        match picker {
            Ok(picker) => self.model_picker = Some(picker),
            Err(error) => self.push_system_notice(error.to_string()),
        }
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
        self.session_id = Some(session_id.clone());
        self.set_current_model_selection(&session.model_selection);
        self.session_usage = session.usage.tokens.clone();
        self.session_context_usage = session.usage.context.clone();
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
                        Ok(Some(_)) => Some(client.recover_timeline(&session_id, None).await),
                        Ok(None) => None,
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
        let model_ref = selection.model_ref;
        let reasoning_effort = selection.reasoning_effort;
        let model_key = format!("{}:{}", model_ref.provider, model_ref.id);
        let model_selection = talos_sdk::ModelSelection {
            model_ref: model_ref.clone(),
            reasoning_effort,
        };
        if self.session_id.is_none() {
            self.set_current_model_selection(&model_selection);
            self.close_model_picker();
            self.push_command_result(format!(
                "Model changed to {}",
                self.current_model_status_label()
            ));
            self.push_right_event(format!("draft model set: {model_key}"));
            return;
        }
        let Some(client) = self.gateway_client.clone() else {
            self.close_model_picker();
            return;
        };
        let session_id = self.session_id.clone().expect("session checked above");
        let requested = model_key.clone();
        let _ = self.spawn_command_task(
            format!("model picker switching to: {model_key}"),
            async move {
                let result = client
                    .update_session_model_selection(&session_id, &model_selection)
                    .await;
                CommandUpdate::ModelPickerSwitched { requested, result }
            },
        );
    }

    pub(crate) fn close_session_command(&mut self) -> TalosResult<()> {
        if self.close_in_progress {
            return Ok(());
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::internal("missing gateway client".to_string()));
        };
        let session_id = self.session_id.clone();
        let closing_session_id = session_id.clone().unwrap_or_else(|| "none".to_string());
        let (tx, rx) = mpsc::channel(1);
        self.pending_close_update = Some(rx);
        self.close_in_progress = true;
        self.close_started_at = Some(Instant::now());
        self.close_slow_notice_reported = false;
        self.closing_session_id = Some(closing_session_id.clone());
        self.connection_status = "closing".to_string();
        self.show_status_hint(
            "Closing session and flushing memory.",
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

    pub(crate) fn can_force_close(&self) -> bool {
        self.close_started_at
            .map(|started_at| started_at.elapsed() >= SESSION_CLOSE_FORCE_TIMEOUT)
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
) -> SessionCloseUpdate {
    let close_result = if let Some(session_id) = session_id.as_deref() {
        client
            .close_session(session_id)
            .await
            .map(|_| session_id.to_string())
    } else {
        Ok(fallback_session_id)
    };
    let shutdown_result = client.shutdown().await;
    let client_shutdown_completed = shutdown_result.is_ok();
    let result = match (close_result, shutdown_result) {
        (Ok(session_id), Ok(())) => Ok(session_id),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(close_error), Err(shutdown_error)) => Err(TalosError::internal(format!(
            "close session failed: {close_error}; shutdown failed: {shutdown_error}"
        ))),
    };
    SessionCloseUpdate {
        result,
        client_shutdown_completed,
    }
}
