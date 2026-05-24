use std::time::Instant;

use talos_error::{TalosError, TalosResult};
use talos_sdk::{TalosInputItem, TalosMessageRequest};
use tokio::sync::mpsc;
use tracing::error;

use crate::types::ChatRole;
use crate::utils::summarize_for_queue;
use crate::TuiApp;

impl TuiApp {
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
            workspace_id: None,
        };

        self.abort_pending_stream_task();
        self.in_flight = true;
        self.received_output_delta = false;
        self.transcript.reset_for_new_stream();
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
        self.last_stream_event_id = None;
        self.timeline_items.clear();

        if reset_retry_state {
            let user_idx = self.push_block(ChatRole::User, user_message.clone());
            self.active_user_idx = Some(user_idx);
        }

        let (tx, rx) = mpsc::channel(1);
        self.pending_stream_connect = Some(rx);
        self.push_right_event(format!("message dispatched to session {}", session_id));

        let handle = tokio::spawn(async move {
            let result = client.send_timeline_events(&session_id, &request).await;
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
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.last_stream_event_id = None;
        self.timeline_items.clear();
        self.stream_started_at = None;
        self.last_stream_event_at = None;
        self.active_message = None;
        self.transcript.finalize(&mut self.chat_blocks);
        self.rebuild_chat_history();
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
}
