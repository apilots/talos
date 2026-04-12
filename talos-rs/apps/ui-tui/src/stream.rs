use std::time::{Duration, Instant};

use talos_client::{
    TalosContentItem, TalosEvent, TalosEventPayload, TalosInputItem, TalosMessageRequest,
};
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosEventType;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tracing::error;

use crate::types::{AppState, ChatRole, PendingApprovalState, RemoteCommandUpdate};
use crate::utils::{canonical_event_label, summarize_for_queue};
use crate::{TuiApp, MAX_RIGHT_EVENTS, SPINNER_FRAMES, STREAM_STALL_TIMEOUT};

impl TuiApp {
    pub fn poll_stream_events(&mut self) {
        let mut should_drop_stream = false;

        loop {
            let Some(stream) = self.pending_stream.as_mut() else {
                break;
            };

            match stream.try_recv() {
                Ok(event) => {
                    self.last_stream_event_at = Some(Instant::now());
                    self.handle_stream_event(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    should_drop_stream = true;
                    break;
                }
            }
        }

        if should_drop_stream {
            self.finish_active_stream("stream disconnected", true, false);
        }
    }

    pub(crate) async fn poll_stream_connect(&mut self) {
        let mut connect_result: Option<TalosResult<mpsc::Receiver<TalosEvent>>> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_stream_connect.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    connect_result = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    connect_result = Some(Err(TalosError::ClientError(
                        "stream connect channel dropped".to_string(),
                    )));
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_stream_connect = None;
            self.pending_stream_task = None;
        }

        match connect_result {
            Some(Ok(receiver)) => {
                self.pending_stream = Some(receiver);
                self.last_stream_event_at = Some(Instant::now());
                self.push_right_event("stream channel connected".to_string());
            }
            Some(Err(err)) => {
                let retry_message = self.active_message_raw.clone();
                let can_retry_once = self.active_retry_count == 0 && retry_message.is_some();
                self.discard_active_pending_blocks();
                self.finish_active_stream("stream connect failed", true, true);

                if can_retry_once {
                    if let Some(message) = retry_message {
                        self.active_retry_count = 1;
                        self.show_status_hint(
                            "Failed to open stream. Retrying once...",
                            Duration::from_secs(3),
                        );
                        if let Err(retry_err) =
                            self.start_stream_for_message_internal(message, false).await
                        {
                            self.active_message_raw = None;
                            self.active_retry_count = 0;
                            self.push_block(
                                ChatRole::System,
                                format!("Failed to start stream: {}", retry_err),
                            );
                            self.push_right_event("stream connect retry failed".to_string());
                        }
                    }
                } else {
                    self.active_message_raw = None;
                    self.active_retry_count = 0;
                    self.push_block(ChatRole::System, format!("Failed to start stream: {}", err));
                    self.push_right_event("queued dispatch dropped".to_string());
                }
            }
            None => {}
        }
    }

    pub(crate) fn poll_session_connect(&mut self) {
        let mut connect_result: Option<TalosResult<String>> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_session_connect.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    connect_result = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    connect_result = Some(Err(TalosError::ClientError(
                        "session connect channel dropped".to_string(),
                    )));
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_session_connect = None;
            self.pending_session_task = None;
        }

        match connect_result {
            Some(Ok(session_id)) => {
                self.session_id = Some(session_id.clone());
                self.connection_status = "connected".to_string();
                self.push_right_event(format!("session ready: {}", session_id));
            }
            Some(Err(err)) => {
                self.connection_status = "error".to_string();
                self.push_block(
                    ChatRole::System,
                    format!("Failed to create remote session: {}", err),
                );
                self.push_right_event("session create failed".to_string());
            }
            None => {}
        }
    }

    pub(crate) fn poll_command_updates(&mut self) {
        let mut update: Option<RemoteCommandUpdate> = None;
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
                        "Remote command channel dropped unexpectedly.".to_string(),
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
            RemoteCommandUpdate::ModelsLoaded(result) => match result {
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
            RemoteCommandUpdate::StatusLoaded(result) => match result {
                Ok(session_info) => {
                    let mut lines = vec![
                        "mode: Gateway".to_string(),
                        format!("model: {}", self.current_model),
                        format!("directory: {}", self.startup_directory.display()),
                        format!("connection_mode: {}", self.connection_mode),
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
                    if let Some(info) = session_info {
                        self.apply_session_snapshot(&info);
                        lines.push(format!("server.model: {}", info.config.llm.model));
                        lines.push(format!("server.messages: {}", info.state.message_count));
                        lines.push(format!(
                            "server.usage.total: {}",
                            info.state.usage.total_tokens
                        ));
                    }
                    self.push_block(ChatRole::System, lines.join("\n"));
                    self.push_right_event("status loaded".to_string());
                }
                Err(err) => {
                    self.push_block(ChatRole::System, format!("Failed to fetch status: {}", err));
                    self.push_right_event("status load failed".to_string());
                }
            },
            RemoteCommandUpdate::ModelSwitched { requested, result } => match result {
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
            RemoteCommandUpdate::HistoryLoaded(result) => match result {
                Ok(sessions) => {
                    let mut sessions = sessions;
                    sessions.sort_by(|left, right| {
                        right
                            .state
                            .last_activity
                            .cmp(&left.state.last_activity)
                            .then(right.state.created_at.cmp(&left.state.created_at))
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
            RemoteCommandUpdate::ModelPickerSwitched { requested, result } => match result {
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
            RemoteCommandUpdate::SessionTurnsLoaded { session, result } => match result {
                Ok(turns) => {
                    let sid = &session.context.session_id;
                    let model = &session.config.llm.model;
                    let title = &session.title;
                    self.push_block(
                        ChatRole::System,
                        format!("Session: {}  ({})\nModel: {}", title, sid, model),
                    );
                    for (turn_index, turn) in turns.iter().enumerate() {
                        for (item_index, item) in turn.output.iter().enumerate() {
                            self.replay_history_item(item, turn_index as u64, item_index as u64);
                        }
                    }
                    self.message_history_index = None;
                    self.push_right_event(format!("turns loaded: {}", turns.len()));
                }
                Err(err) => {
                    let sid = &session.context.session_id;
                    self.push_block(
                        ChatRole::System,
                        format!(
                            "Session: {}  ({})\nFailed to load history: {}",
                            session.title, sid, err
                        ),
                    );
                    self.push_right_event("turns load failed".to_string());
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

    pub async fn on_tick(&mut self) {
        self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
        self.stripe_phase = self.stripe_phase.wrapping_add(1);

        if let Some((_, expires_at)) = &self.status_hint {
            if Instant::now() >= *expires_at {
                self.status_hint = None;
            }
        }

        self.poll_session_connect();
        self.poll_stream_connect().await;
        self.poll_stream_events();
        self.poll_command_updates();
        self.poll_close_updates();
        self.poll_close_timeout();

        if self.in_flight {
            if let Some(last_event_at) = self.last_stream_event_at {
                if last_event_at.elapsed() > STREAM_STALL_TIMEOUT {
                    let retry_message = self.active_message_raw.clone();
                    let can_retry_once = self.active_retry_count == 0 && retry_message.is_some();
                    self.finish_active_stream("stream stalled and timed out", true, true);

                    if can_retry_once {
                        if let Some(message) = retry_message {
                            self.active_retry_count = 1;
                            self.show_status_hint(
                                "Current response timed out. Retrying once...",
                                Duration::from_secs(3),
                            );
                            if let Err(err) =
                                self.start_stream_for_message_internal(message, false).await
                            {
                                self.active_message_raw = None;
                                self.active_retry_count = 0;
                                self.push_block(ChatRole::System, format!("Retry failed: {}", err));
                                self.show_status_hint(
                                    "Retry failed. Moving to next queued message.",
                                    Duration::from_secs(4),
                                );
                            }
                        }
                    } else {
                        self.active_message_raw = None;
                        self.active_retry_count = 0;
                        self.show_status_hint(
                            "Current response timed out. Moving to next queued message.",
                            Duration::from_secs(4),
                        );
                    }
                }
            }
        }

        if !self.in_flight && !self.close_in_progress {
            self.dispatch_next_queued_message().await;
        }
    }

    pub(crate) async fn start_stream_for_message(
        &mut self,
        user_message: String,
    ) -> TalosResult<()> {
        self.start_stream_for_message_internal(user_message, true)
            .await
    }

    pub(crate) async fn start_stream_for_message_internal(
        &mut self,
        user_message: String,
        reset_retry_state: bool,
    ) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            self.push_block(ChatRole::System, "Gateway client unavailable.".to_string());
            return Ok(());
        };

        let Some(session_id) = self.session_id.clone() else {
            return Err(TalosError::ClientError("session is not ready".to_string()));
        };
        let active_summary = summarize_for_queue(&user_message);
        let request = TalosMessageRequest {
            input: vec![TalosInputItem::Text {
                text: user_message.clone(),
            }],
            stream: Some(true),
            workspace_id: None,
        };

        self.abort_pending_stream_task();
        self.in_flight = true;
        self.received_output_delta = false;
        self.pending_assistant_output.clear();
        self.stream_has_tool_calls = false;
        self.active_message = Some(active_summary);
        if reset_retry_state {
            self.active_retry_count = 0;
            self.active_message_raw = Some(user_message.clone());
        }
        self.stream_started_at = Some(Instant::now());
        self.last_stream_event_at = Some(Instant::now());
        self.pending_stream = None;
        self.pending_stream_connect = None;
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;

        if reset_retry_state {
            let user_idx = self.push_block(ChatRole::User, user_message.clone());
            self.active_user_idx = Some(user_idx);
        }

        let thinking_idx = self.push_block(ChatRole::Thinking, String::new());
        self.active_thinking_idx = Some(thinking_idx);
        self.active_assistant_idx = None;

        let (tx, rx) = mpsc::channel(1);
        self.pending_stream_connect = Some(rx);
        self.push_right_event(format!("message dispatched to session {}", session_id));

        let handle = tokio::spawn(async move {
            let result = client.send_message_stream(&session_id, &request).await;
            if tx.send(result).await.is_err() {
                error!(
                    session_id = %session_id,
                    "failed to deliver stream connection result to TUI task"
                );
            }
        });
        self.pending_stream_task = Some(handle);

        Ok(())
    }

    pub(crate) fn handle_stream_event(&mut self, event: TalosEvent) {
        self.trace_event_count = self.trace_event_count.saturating_add(1);
        let is_tool_lifecycle_event = Self::is_tool_lifecycle_event(&event.event_type);

        if let Some(session_id) = self.session_id.as_deref() {
            if event.session_id != session_id {
                self.push_right_event(format!(
                    "ignored event from other session: {}",
                    event.session_id
                ));
                return;
            }
        }

        match self.active_stream_message_id.as_deref() {
            Some(active_id) if event.message_id != active_id && !is_tool_lifecycle_event => {
                self.push_right_event(format!(
                    "ignored stale event for {} (active {})",
                    event.message_id, active_id
                ));
                return;
            }
            None => {
                self.active_stream_message_id = Some(event.message_id.clone());
            }
            _ => {}
        }

        if let Some(last_seq) = self.last_stream_sequence {
            if event.sequence < last_seq && !is_tool_lifecycle_event {
                self.push_right_event(format!(
                    "ignored out-of-order event seq {} < {}",
                    event.sequence, last_seq
                ));
                return;
            }
        }
        self.last_stream_sequence = Some(event.sequence);
        self.request_follow_bottom();

        if event.event_type != TalosEventType::MessageOutputDelta {
            self.push_right_event(format!(
                "{} #{}",
                canonical_event_label(&event.event_type),
                event.sequence
            ));
        }

        match (&event.event_type, &event.payload) {
            (TalosEventType::MessageStarted, TalosEventPayload::MessageStarted(_)) => {}
            (
                TalosEventType::MessageReasoningDelta,
                TalosEventPayload::MessageReasoningDelta(payload),
            ) => {
                if !payload.delta.is_empty() {
                    self.append_thinking(&payload.delta);
                }
            }
            (
                TalosEventType::MessageExecutionSummary,
                TalosEventPayload::MessageExecutionSummary(payload),
            ) => {
                if !payload.text.trim().is_empty() {
                    self.push_block(
                        ChatRole::System,
                        format!(
                            "Execution summary [{} #{}]: {}",
                            payload.step_id, payload.iteration, payload.text
                        ),
                    );
                }
            }
            (
                TalosEventType::MessageOutputDelta,
                TalosEventPayload::MessageOutputDelta(payload),
            ) => {
                if self.stream_has_tool_calls {
                    self.pending_assistant_output.push_str(&payload.delta);
                } else {
                    self.append_assistant(&payload.delta);
                }
                self.received_output_delta = true;
            }
            (
                TalosEventType::MessageToolCallStarted,
                TalosEventPayload::MessageToolCallStarted(payload),
            ) => {
                self.start_tool_output_buffering();
                self.push_right_event(format!(
                    "tool started [{}]: {}",
                    payload.step_id, payload.name
                ));
                self.tool_events
                    .push(format!("started [{}]: {}", payload.step_id, payload.name));
                self.push_tool_call_block(
                    payload.call_id.clone(),
                    payload.name.clone(),
                    payload.arguments.clone(),
                    payload.iteration,
                    payload.item_index,
                    payload.timing_ms,
                );
            }
            (
                TalosEventType::MessageToolCallCompleted,
                TalosEventPayload::MessageToolCallCompleted(payload),
            ) => {
                self.push_right_event(format!(
                    "tool completed: {} ({})",
                    payload.call_id, payload.tool
                ));
                self.tool_events
                    .push(format!("completed: {} ({})", payload.call_id, payload.tool));
                let output = payload
                    .content
                    .iter()
                    .filter_map(|item| match item {
                        TalosContentItem::Text { text } => Some(text.clone()),
                        TalosContentItem::Json { value } => Some(value.to_string()),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.complete_tool_call_block(
                    &payload.call_id,
                    output,
                    !payload.is_error,
                    payload.timing_ms,
                );
            }
            (
                TalosEventType::MessageToolCallPendingApproval,
                TalosEventPayload::MessageToolCallPendingApproval(payload),
            ) => {
                self.push_right_event(format!(
                    "approval needed: {} ({})",
                    payload.tool_name, payload.call_id
                ));
                self.pending_approval = Some(PendingApprovalState {
                    call_id: payload.call_id.clone(),
                    tool_name: payload.tool_name.clone(),
                    prompt: payload.prompt.clone(),
                    arguments: payload.arguments.clone(),
                });
            }
            (TalosEventType::MessageCompleted, TalosEventPayload::MessageCompleted(payload)) => {
                self.session_usage.input_tokens = self
                    .session_usage
                    .input_tokens
                    .saturating_add(payload.usage.input_tokens);
                self.session_usage.output_tokens = self
                    .session_usage
                    .output_tokens
                    .saturating_add(payload.usage.output_tokens);
                self.session_usage.total_tokens = self
                    .session_usage
                    .total_tokens
                    .saturating_add(payload.usage.total_tokens);
                self.finish_active_stream("stream completed", false, false);
            }
            (TalosEventType::MessageError, TalosEventPayload::MessageError(payload)) => {
                self.push_block(ChatRole::System, format!("Stream error: {}", payload.error));
                self.finish_active_stream("stream error", true, false);
            }
            _ => {}
        }
    }

    pub(crate) fn start_tool_output_buffering(&mut self) {
        if self.stream_has_tool_calls {
            return;
        }

        self.stream_has_tool_calls = true;
        if let Some(idx) = self.active_assistant_idx {
            let mut should_remove = false;
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                if !block.content.is_empty() {
                    self.pending_assistant_output.push_str(&block.content);
                    block.content.clear();
                }
                should_remove = block.content.is_empty();
            }
            if should_remove {
                self.remove_chat_block(idx);
                self.rebuild_chat_history();
                self.active_assistant_idx = None;
            } else {
                self.sync_history_entry(idx);
            }
        }
    }

    pub(crate) fn flush_pending_assistant_output(&mut self) {
        if self.pending_assistant_output.is_empty() {
            return;
        }
        let buffered = std::mem::take(&mut self.pending_assistant_output);
        self.append_assistant(&buffered);
    }

    pub(crate) fn append_assistant(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = if let Some(idx) = self.active_assistant_idx {
            idx
        } else {
            let idx = self.push_block(ChatRole::Assistant, String::new());
            self.active_assistant_idx = Some(idx);
            idx
        };

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content.push_str(text);
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    pub(crate) fn append_thinking(&mut self, text: &str) {
        let idx = if let Some(idx) = self.active_thinking_idx {
            idx
        } else {
            let idx = self.push_block(ChatRole::Thinking, String::new());
            self.active_thinking_idx = Some(idx);
            idx
        };

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content.push_str(text);
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    pub(crate) fn push_right_event(&mut self, event: String) {
        self.right_panel_events.push(event);
        if self.right_panel_events.len() > MAX_RIGHT_EVENTS {
            let overflow = self.right_panel_events.len() - MAX_RIGHT_EVENTS;
            self.right_panel_events.drain(0..overflow);
        }
        if self.tool_events.len() > MAX_RIGHT_EVENTS {
            let overflow = self.tool_events.len() - MAX_RIGHT_EVENTS;
            self.tool_events.drain(0..overflow);
        }
    }

    pub(crate) fn abort_pending_stream_task(&mut self) {
        if let Some(handle) = self.pending_stream_task.take() {
            handle.abort();
        }
    }

    pub(crate) fn finish_active_stream(
        &mut self,
        reason: &str,
        is_error: bool,
        keep_active_payload: bool,
    ) {
        let completed_round_timing_ms = if !is_error {
            self.stream_started_at
                .map(|started_at| started_at.elapsed().as_millis() as u64)
        } else {
            None
        };
        if !is_error {
            self.flush_pending_assistant_output();
        } else {
            self.pending_assistant_output.clear();
        }
        if let (Some(idx), Some(duration_ms)) = (self.active_user_idx, completed_round_timing_ms) {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                block.round_timing_ms = Some(duration_ms);
            }
        }
        self.in_flight = false;
        self.pending_stream_connect = None;
        self.pending_stream = None;
        self.abort_pending_stream_task();
        self.active_user_idx = None;
        self.active_thinking_idx = None;
        self.active_assistant_idx = None;
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.stream_started_at = None;
        self.last_stream_event_at = None;
        self.active_message = None;
        self.active_tool_calls.clear();
        self.stream_has_tool_calls = false;
        if !keep_active_payload {
            self.active_message_raw = None;
            self.active_retry_count = 0;
        }
        self.push_right_event(if is_error {
            format!("stream finished with error: {}", reason)
        } else {
            format!("stream finished: {}", reason)
        });
    }

    pub(crate) fn sync_history_entry(&mut self, idx: usize) {
        if let Some(block) = self.chat_blocks.get(idx) {
            let entry = format!("{}\n{}", block.role.as_str(), block.content);
            if idx < self.chat_history.len() {
                self.chat_history[idx] = entry;
            } else {
                self.chat_history.push(entry);
            }
        }
    }
}
