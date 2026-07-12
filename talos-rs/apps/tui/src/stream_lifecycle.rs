use std::time::Instant;

use talos_error::TalosResult;
use talos_sdk::SkillSuggestion;
use tokio::sync::mpsc;
use tracing::error;

use crate::utils::summarize_for_queue;
use crate::{TerminalTurnRequest, TuiApp};

impl TuiApp {
    pub(crate) async fn start_stream_for_message_with_skills(
        &mut self,
        user_message: String,
        skills: Vec<SkillSuggestion>,
    ) -> TalosResult<()> {
        self.start_stream_for_message_internal(user_message, skills, true)
            .await
    }

    pub(crate) async fn start_stream_for_message_internal(
        &mut self,
        user_message: String,
        skills: Vec<SkillSuggestion>,
        reset_retry_state: bool,
    ) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            self.push_system_notice("Gateway client unavailable.");
            return Ok(());
        };

        let session_id = match self.session_id.clone() {
            Some(session_id) => session_id,
            None => {
                let session_id = uuid::Uuid::new_v4().to_string();
                self.session_id = Some(session_id.clone());
                session_id
            }
        };
        let active_summary = summarize_for_queue(&user_message);
        let request = TerminalTurnRequest {
            session_id: session_id.clone(),
            message: user_message.clone(),
            skills: skills.clone(),
            context: Some(self.build_talos_context()),
            model_selection: self.current_model_selection(),
            current_directory: Some(self.startup_directory.clone()),
            client_request_id: None,
        }
        .build();

        self.abort_pending_stream_task();
        self.in_flight = true;
        self.received_output_delta = false;
        self.active_message = Some(active_summary);
        if reset_retry_state {
            self.active_retry_count = 0;
            self.active_message_raw = Some(user_message.clone());
            self.active_message_skills = skills.clone();
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
            self.render_runtime.append_user_input(user_message.clone());
            self.request_chat_activity();
        }

        let (tx, rx) = mpsc::channel(1);
        self.pending_stream_connect = Some(rx);
        self.push_right_event(format!("message dispatched to session {}", session_id));

        let handle = tokio::spawn(async move {
            let result = client.start_turn_stream(request).await;
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
        self.in_flight = false;
        self.turn_metrics_live = false;
        self.pending_stream_connect = None;
        self.pending_stream = None;
        self.abort_pending_stream_task();
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.last_stream_event_id = None;
        self.timeline_items.clear();
        self.stream_started_at = None;
        self.last_stream_event_at = None;
        self.active_message = None;
        if !keep_active_payload {
            self.active_message_raw = None;
            self.active_message_skills.clear();
            self.active_retry_count = 0;
        }
        self.push_right_event(if is_error {
            format!("stream finished with error: {}", reason)
        } else {
            format!("stream finished: {}", reason)
        });
    }
}
