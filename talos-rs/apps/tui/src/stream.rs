use std::time::{Duration, Instant};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sdk::TimelineEvent;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use crate::{TuiApp, SPINNER_FRAMES, STREAM_STALL_TIMEOUT};

fn is_active_turn_conflict(error: &TalosError) -> bool {
    error.is_code(ErrorCode::ActiveTurnRunning)
}

impl TuiApp {
    pub fn poll_stream_events(&mut self) {
        let mut should_drop_stream = false;

        while let Some(stream) = self.pending_stream.as_mut() {
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
        let mut connect_result: Option<TalosResult<mpsc::Receiver<TimelineEvent>>> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_stream_connect.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    connect_result = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    connect_result = Some(Err(TalosError::internal(
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
                if is_active_turn_conflict(&err) && self.reconnect_active_stream().await {
                    return;
                }
                let retry_message = self.active_message_raw.clone();
                let retry_skills = self.active_message_skills.clone();
                let retry_file_ids = self.active_message_file_ids.clone();
                let can_retry_once = self.active_retry_count == 0 && retry_message.is_some();
                self.finish_active_stream("stream connect failed", true, true);

                if can_retry_once {
                    if let Some(message) = retry_message {
                        self.active_retry_count = 1;
                        self.show_status_hint(
                            "Failed to open stream. Retrying once...",
                            Duration::from_secs(3),
                        );
                        if let Err(retry_err) = self
                            .start_stream_for_message_internal(
                                message,
                                retry_skills,
                                retry_file_ids,
                                false,
                            )
                            .await
                        {
                            self.active_message_raw = None;
                            self.active_message_skills.clear();
                            self.active_message_file_ids.clear();
                            self.active_retry_count = 0;
                            self.push_system_notice(format!(
                                "Failed to start stream: {}",
                                retry_err
                            ));
                            self.push_right_event("stream connect retry failed".to_string());
                        }
                    }
                } else {
                    self.active_message_raw = None;
                    self.active_message_skills.clear();
                    self.active_message_file_ids.clear();
                    self.active_retry_count = 0;
                    self.push_system_notice(format!("Failed to start stream: {}", err));
                    self.push_right_event("stream connect failed".to_string());
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
                    connect_result = Some(Err(TalosError::internal(
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
                self.push_system_notice(format!("Failed to create session: {}", err));
                self.push_right_event("session create failed".to_string());
            }
            None => {}
        }
    }

    pub async fn on_tick(&mut self) {
        self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();

        if let Some((_, expires_at)) = &self.status_hint {
            if Instant::now() >= *expires_at {
                self.status_hint = None;
            }
        }

        self.poll_session_connect();
        self.poll_stream_connect().await;
        self.poll_stream_events();
        self.poll_session_usage_refresh();
        self.poll_command_updates();
        self.poll_close_updates();
        self.poll_close_progress();

        if self.in_flight {
            if let Some(last_event_at) = self.last_stream_event_at {
                if last_event_at.elapsed() > STREAM_STALL_TIMEOUT
                    && !self.reconnect_active_stream().await
                {
                    self.finish_active_stream("stream stalled and timed out", true, true);
                    self.active_message_raw = None;
                    self.active_retry_count = 0;
                    self.show_status_hint("Current response timed out.", Duration::from_secs(4));
                }
            }
        }

        self.ensure_message_queue_subscription().await;
        self.poll_message_queue_changes().await;
    }

    async fn reconnect_active_stream(&mut self) -> bool {
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            return false;
        };
        let since_event_id = self.last_stream_event_id.clone();
        match client
            .reconnect_active_turn_stream(&session_id, since_event_id)
            .await
        {
            Ok(receiver) => {
                self.pending_stream_connect = None;
                self.pending_stream_task = None;
                self.pending_stream = Some(receiver);
                self.last_stream_event_at = Some(Instant::now());
                self.in_flight = true;
                self.show_status_hint("Reconnected to active turn stream.", Duration::from_secs(3));
                self.push_right_event("active turn stream reconnected".to_string());
                true
            }
            Err(err) => {
                self.push_right_event(format!("active turn reconnect failed: {}", err));
                false
            }
        }
    }
}
