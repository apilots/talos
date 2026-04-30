use talos_client::{TalosContentItem, TalosEvent, TalosEventPayload};
use talos_types::talos::{TalosEventType, TalosToolProgressKind};

use crate::types::{ChatRole, PendingApprovalState, ToolCallStatus};
use crate::TuiApp;

impl TuiApp {
    pub(crate) fn handle_stream_event(&mut self, event: TalosEvent) {
        self.trace_event_count = self.trace_event_count.saturating_add(1);

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
            Some(active_id) if event.message_id != active_id => {
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
            if event.sequence < last_seq {
                self.push_right_event(format!(
                    "ignored out-of-order event seq {} < {}",
                    event.sequence, last_seq
                ));
                return;
            }
        }
        self.last_stream_sequence = Some(event.sequence);
        self.request_follow_bottom();

        match (&event.event_type, &event.payload) {
            (TalosEventType::MessageStarted, TalosEventPayload::MessageStarted(_)) => {}
            (
                TalosEventType::MessageReasoningDelta,
                TalosEventPayload::MessageReasoningDelta(payload),
            ) => {
                if !payload.delta.is_empty() {
                    self.transcript.note_visible_activity();
                    self.append_thinking(&payload.delta);
                }
            }
            (
                TalosEventType::MessageExecutionSummary,
                TalosEventPayload::MessageExecutionSummary(payload),
            ) => {
                if !payload.text.trim().is_empty() {
                    self.push_right_event(format!("execution summary: {}", payload.text.trim()));
                }
            }
            (
                TalosEventType::MessageOutputDelta,
                TalosEventPayload::MessageOutputDelta(payload),
            ) => {
                self.transcript.note_visible_activity();
                self.append_assistant(&payload.delta);
                self.received_output_delta = true;
            }
            (
                TalosEventType::MessageToolCallStarted,
                TalosEventPayload::MessageToolCallStarted(payload),
            ) => {
                self.transcript.note_visible_activity();
                self.finish_live_text_blocks();
                self.push_right_event(format!(
                    "tool started [{}]: {}",
                    payload.step_id, payload.name
                ));
                self.upsert_tool_event(
                    payload.call_id.clone(),
                    format!("queued: {}", payload.name),
                );
                self.push_tool_call_block(
                    payload.call_id.clone(),
                    payload.name.clone(),
                    payload.arguments.clone(),
                    payload.iteration,
                    payload.item_index,
                    payload.timing_ms,
                );
                self.transcript
                    .record_stream_key(0, payload.item_index as usize, &payload.call_id);
                self.update_tool_call_status(&payload.call_id, ToolCallStatus::Queued);
            }
            (
                TalosEventType::MessageToolCallArgsDelta,
                TalosEventPayload::MessageToolCallArgsDelta(payload),
            ) => {
                if let Some(call_id) = payload.call_id.as_deref() {
                    self.transcript.record_stream_key(
                        payload.stream_key.round,
                        payload.stream_key.index,
                        call_id,
                    );
                }
            }
            (
                TalosEventType::MessageToolCallQueued,
                TalosEventPayload::MessageToolCallQueued(payload),
            ) => {
                self.transcript.note_visible_activity();
                self.push_right_event(format!(
                    "tool queued: {} ({})",
                    payload.call_id,
                    crate::utils::display_tool_name(&payload.tool_name)
                ));
                self.upsert_tool_event(
                    payload.call_id.clone(),
                    format!(
                        "queued: {}",
                        crate::utils::display_tool_name(&payload.tool_name)
                    ),
                );
                self.transcript.record_stream_key(
                    payload.stream_key.round,
                    payload.stream_key.index,
                    &payload.call_id,
                );
                self.update_tool_call_status(&payload.call_id, ToolCallStatus::Queued);
            }
            (
                TalosEventType::MessageToolCallExecuting,
                TalosEventPayload::MessageToolCallExecuting(payload),
            ) => {
                self.transcript.note_visible_activity();
                self.push_right_event(format!(
                    "tool executing: {} ({})",
                    payload.call_id,
                    crate::utils::display_tool_name(&payload.tool_name)
                ));
                self.upsert_tool_event(
                    payload.call_id.clone(),
                    format!(
                        "running: {}",
                        crate::utils::display_tool_name(&payload.tool_name)
                    ),
                );
                self.transcript.record_stream_key(
                    payload.stream_key.round,
                    payload.stream_key.index,
                    &payload.call_id,
                );
                self.update_tool_call_status(&payload.call_id, ToolCallStatus::Running);
            }
            (
                TalosEventType::MessageToolCallProgressDelta,
                TalosEventPayload::MessageToolCallProgressDelta(payload),
            ) => {
                self.transcript.note_visible_activity();
                self.append_tool_call_progress(
                    &payload.call_id,
                    Self::format_tool_progress_line(payload.progress_kind, &payload.message),
                );
            }
            (
                TalosEventType::MessageToolCallCompleted,
                TalosEventPayload::MessageToolCallCompleted(payload),
            ) => {
                self.push_right_event(format!(
                    "tool completed: {} ({})",
                    payload.call_id,
                    crate::utils::display_tool_name(&payload.tool)
                ));
                self.upsert_tool_event(
                    payload.call_id.clone(),
                    format!("done: {}", crate::utils::display_tool_name(&payload.tool)),
                );
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
                self.clear_live_tool_progress(&payload.call_id);
            }
            (
                TalosEventType::MessageToolCallPendingApproval,
                TalosEventPayload::MessageToolCallPendingApproval(payload),
            ) => {
                self.transcript.note_visible_activity();
                self.push_right_event(format!(
                    "approval needed: {} ({})",
                    crate::utils::display_tool_name(&payload.tool_name),
                    payload.call_id
                ));
                self.upsert_tool_event(
                    payload.call_id.clone(),
                    format!(
                        "approval: {}",
                        crate::utils::display_tool_name(&payload.tool_name)
                    ),
                );
                self.pending_approval = Some(PendingApprovalState {
                    call_id: payload.call_id.clone(),
                    tool_name: payload.tool_name.clone(),
                    prompt: payload.prompt.clone(),
                    arguments: payload.arguments.clone(),
                });
                self.push_tool_call_block(
                    payload.call_id.clone(),
                    payload.tool_name.clone(),
                    payload.arguments.clone(),
                    0,
                    0,
                    None,
                );
                self.update_tool_call_status(&payload.call_id, ToolCallStatus::PendingApproval);
                self.append_tool_call_progress(
                    &payload.call_id,
                    Self::format_tool_progress_line(
                        TalosToolProgressKind::Status,
                        "waiting for approval",
                    ),
                );
            }
            (TalosEventType::MessageCompleted, TalosEventPayload::MessageCompleted(payload)) => {
                self.session_usage.accumulate(&payload.usage);
                self.finish_active_stream("stream completed", false, false);
            }
            (TalosEventType::MessageError, TalosEventPayload::MessageError(payload)) => {
                self.push_block(ChatRole::System, format!("Stream error: {}", payload.error));
                self.finish_active_stream("stream error", true, false);
            }
            _ => {}
        }
    }

    fn format_tool_progress_line(kind: TalosToolProgressKind, message: &str) -> String {
        match kind {
            TalosToolProgressKind::Status => format!("status: {}", message),
            TalosToolProgressKind::Stdout => format!("stdout: {}", message),
            TalosToolProgressKind::Stderr => format!("stderr: {}", message),
        }
    }
}
