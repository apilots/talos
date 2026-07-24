use std::time::Instant;

use talos_sdk::{
    AgentEventPayload, ContextCompactionOutcome, ContextCompactionStage, SessionHistorySnapshot,
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TimelineEventEnvelope, TimelineRecoveryResponse,
};
use tokio::sync::mpsc;
use tracing::error;

use crate::render_runtime::render::history::timeline_item_to_blocks;
use crate::render_runtime::transcript::block::TranscriptBlockKind;
use crate::TuiApp;

impl TuiApp {
    pub(crate) fn handle_stream_event(&mut self, envelope: TimelineEventEnvelope) {
        self.trace_event_count = self.trace_event_count.saturating_add(1);

        if let Some(session_id) = self.session_id.as_deref() {
            if envelope.session_id != session_id {
                self.push_right_event(format!(
                    "ignored event from other session: {}",
                    envelope.session_id
                ));
                return;
            }
        }

        match self.active_stream_message_id.as_deref() {
            Some(active_id) if envelope.message_id != active_id => {
                self.push_right_event(format!(
                    "ignored stale event for {} (active {})",
                    envelope.message_id, active_id
                ));
                return;
            }
            None => {
                self.active_stream_message_id = Some(envelope.message_id.clone());
            }
            _ => {}
        }

        let event = envelope.event;
        if let Some(last_seq) = self.last_stream_sequence {
            if event.sequence <= last_seq {
                self.push_right_event(format!(
                    "ignored duplicate or out-of-order event seq {} <= {}",
                    event.sequence, last_seq
                ));
                return;
            }
        }
        self.last_stream_sequence = Some(event.sequence);
        self.last_stream_event_id = Some(envelope.event_id);
        self.request_chat_activity();

        self.render_runtime
            .apply_agent_event_payload(event.payload.clone());

        match event.payload {
            AgentEventPayload::TurnStarted(_) => {
                self.turn_metrics = None;
                self.turn_metrics_live = false;
            }
            AgentEventPayload::ItemStarted(payload) => {
                self.handle_item_started(payload.item);
            }
            AgentEventPayload::ItemDelta(payload) => {
                self.handle_item_delta(&payload.item_id, payload.delta);
            }
            AgentEventPayload::ItemCompleted(payload) => {
                self.handle_item_completed(
                    &payload.item_id,
                    payload.status,
                    payload.completed_at_ms,
                    payload.final_content,
                );
            }
            AgentEventPayload::AssistantTextCommitted(payload) => {
                self.handle_assistant_text_committed(&payload.item_id, payload.phase);
            }
            AgentEventPayload::TurnMetrics(payload) => {
                self.turn_metrics = Some(payload.metrics);
                self.turn_metrics_live = true;
            }
            AgentEventPayload::TurnCompleted(payload) => {
                self.session_usage.accumulate(&payload.usage);
                if let Some(metrics) = payload.metrics {
                    self.turn_metrics = Some(metrics);
                }
                self.turn_metrics_live = false;
                self.finish_active_stream("stream completed", false, false);
            }
            AgentEventPayload::TurnError(payload) => {
                self.turn_metrics_live = false;
                let _ = payload;
                self.finish_active_stream("stream error", true, false);
            }
        }
    }

    pub(crate) fn replay_recovery_snapshot(&mut self, response: TimelineRecoveryResponse) -> usize {
        let snapshot = response.snapshot;
        let mut events = snapshot.replay.events;
        events.sort_by_key(|a| a.event_seq);
        let count = events.len();
        for envelope in events {
            if self.active_stream_message_id.as_deref() != Some(envelope.message_id.as_str()) {
                self.active_stream_message_id = None;
                self.last_stream_sequence = None;
                self.last_stream_event_id = None;
                self.timeline_items.clear();
            }
            self.handle_stream_event(envelope);
        }
        if self.active_context_compaction_stage().is_some() {
            self.in_flight = true;
            self.stream_started_at = Some(Instant::now());
            self.last_stream_event_at = Some(Instant::now());
            if let (Some(client), Some(session_id)) =
                (self.gateway_client.clone(), self.session_id.clone())
            {
                let since_event_id = self.last_stream_event_id.clone();
                let (tx, rx) = mpsc::channel(1);
                self.pending_stream_connect = Some(rx);
                let handle = tokio::spawn(async move {
                    let result = client
                        .reconnect_active_turn_stream(&session_id, since_event_id)
                        .await;
                    if tx.send(result).await.is_err() {
                        error!("failed to deliver recovered stream to TUI task");
                    }
                });
                self.pending_stream_task = Some(handle);
            }
        } else {
            self.active_stream_message_id = None;
            self.last_stream_sequence = None;
            self.last_stream_event_id = None;
            self.timeline_items.clear();
        }
        count
    }

    pub(crate) fn replay_history_snapshot(&mut self, snapshot: SessionHistorySnapshot) -> usize {
        let mut item_count = 0;
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.last_stream_event_id = None;
        self.timeline_items.clear();

        for message in snapshot.messages {
            let round_duration_ms = message
                .metrics
                .as_ref()
                .and_then(|metrics| metrics.duration_ms);
            for item in message.items {
                if self.render_history_item(item, round_duration_ms) {
                    item_count += 1;
                }
            }
        }

        self.timeline_items.clear();
        item_count
    }

    fn render_history_item(
        &mut self,
        item: TalosTimelineItem,
        round_duration_ms: Option<u64>,
    ) -> bool {
        let transcript_blocks = timeline_item_to_blocks(&item)
            .into_iter()
            .map(|block| {
                if block.kind == TranscriptBlockKind::UserInput {
                    return match round_duration_ms {
                        Some(round_duration_ms) => block.with_round_duration_ms(round_duration_ms),
                        None => block,
                    };
                }
                block
            })
            .collect::<Vec<_>>();
        let rendered_to_transcript = self.render_runtime.append_history(transcript_blocks) > 0;

        match (&item.kind, &item.phase, &item.content) {
            (
                TalosTimelineItemKind::UserInput,
                TalosTimelinePhase::Input,
                TalosTimelineContent::Text { text },
            ) => {
                self.add_message_to_history(text.clone());
                rendered_to_transcript
            }
            (
                TalosTimelineItemKind::UserInput,
                TalosTimelinePhase::Input,
                TalosTimelineContent::UserInput { text, .. },
            ) => {
                if !text.trim().is_empty() {
                    self.add_message_to_history(text.clone());
                }
                rendered_to_transcript
            }
            (
                TalosTimelineItemKind::ExecutionSummary,
                TalosTimelinePhase::System,
                TalosTimelineContent::ExecutionSummary { summary },
            ) => {
                if summary.trim().is_empty() {
                    return false;
                }
                self.push_right_event(format!("execution summary: {}", summary.trim()));
                true
            }
            (
                TalosTimelineItemKind::ContextCompaction,
                TalosTimelinePhase::System,
                TalosTimelineContent::ContextCompaction { .. },
            ) => {
                self.push_right_event(context_compaction_text(&item.content));
                true
            }
            (
                TalosTimelineItemKind::SystemNotice,
                TalosTimelinePhase::System,
                TalosTimelineContent::SystemNotice { message, .. },
            ) => {
                if !message.trim().is_empty() {
                    self.push_right_event(message.trim().to_string());
                }
                rendered_to_transcript
            }
            _ => rendered_to_transcript,
        }
    }

    fn handle_item_started(&mut self, item: TalosTimelineItem) {
        self.timeline_items
            .insert(item.item_id.clone(), item.clone());
        match (&item.kind, &item.phase, &item.content) {
            (
                TalosTimelineItemKind::UserInput,
                TalosTimelinePhase::Input,
                TalosTimelineContent::Text { text },
            ) => {
                let _ = text;
            }
            (
                TalosTimelineItemKind::UserInput,
                TalosTimelinePhase::Input,
                TalosTimelineContent::UserInput { .. },
            ) => {}
            (
                TalosTimelineItemKind::Reasoning,
                TalosTimelinePhase::Thinking,
                TalosTimelineContent::Text { text },
            ) => {
                let _ = text;
            }
            (
                TalosTimelineItemKind::AssistantText,
                TalosTimelinePhase::Unknown | TalosTimelinePhase::Commentary,
                TalosTimelineContent::Text { text },
            ) => {
                let _ = text;
            }
            (
                TalosTimelineItemKind::AssistantText,
                TalosTimelinePhase::FinalAnswer,
                TalosTimelineContent::Text { text },
            ) => {
                if !text.is_empty() {
                    self.received_output_delta = true;
                }
            }
            (
                TalosTimelineItemKind::ToolCall,
                TalosTimelinePhase::Acting,
                TalosTimelineContent::ToolCall { call_id, name, .. },
            ) => {
                self.push_right_event(format!("tool started [{}]: {}", item.item_id, name));
                let _ = call_id;
            }
            (
                TalosTimelineItemKind::ExecutionSummary,
                TalosTimelinePhase::System,
                TalosTimelineContent::ExecutionSummary { summary },
            ) => {
                if !summary.trim().is_empty() {
                    self.push_right_event(format!("execution summary: {}", summary.trim()));
                }
            }
            (
                TalosTimelineItemKind::ContextCompaction,
                TalosTimelinePhase::System,
                TalosTimelineContent::ContextCompaction { .. },
            ) => {
                self.push_right_event(context_compaction_text(&item.content));
            }
            (
                TalosTimelineItemKind::SystemNotice,
                TalosTimelinePhase::System,
                TalosTimelineContent::SystemNotice { message, .. },
            ) if !message.trim().is_empty() => {
                self.push_right_event(message.trim().to_string());
            }
            _ => {}
        }
    }

    fn handle_item_delta(&mut self, item_id: &str, delta: TalosTimelineDelta) {
        match delta {
            TalosTimelineDelta::Text { text } => {
                let Some(item) = self.timeline_items.get(item_id).cloned() else {
                    return;
                };
                if let Some(stored) = self.timeline_items.get_mut(item_id) {
                    if let TalosTimelineContent::Text { text: existing } = &mut stored.content {
                        existing.push_str(&text);
                    }
                }
                self.handle_text_delta(&item, &text);
            }
            TalosTimelineDelta::ToolArguments { arguments_delta } => {
                let _ = self.update_tool_arguments(item_id, &arguments_delta);
            }
            TalosTimelineDelta::ToolProgress { .. } => {}
            TalosTimelineDelta::ToolResult { content, is_error } => {
                self.update_stored_tool_result(item_id, content, is_error);
            }
            TalosTimelineDelta::ToolDynamicRequest {
                call_id, tool_name, ..
            } => {
                self.push_right_event(format!(
                    "dynamic tool requested: {} ({})",
                    crate::utils::display_tool_name(&tool_name),
                    call_id
                ));
            }
            TalosTimelineDelta::ContextCompaction {
                stage,
                outcome,
                metrics,
                message,
            } => {
                let event_text = self.timeline_items.get_mut(item_id).and_then(|item| {
                    if let TalosTimelineContent::ContextCompaction {
                        stage: current_stage,
                        outcome: current_outcome,
                        metrics: current_metrics,
                        message: current_message,
                        ..
                    } = &mut item.content
                    {
                        if let Some(stage) = stage {
                            *current_stage = stage;
                        }
                        if outcome.is_some() {
                            *current_outcome = outcome;
                        }
                        if metrics.is_some() {
                            *current_metrics = metrics;
                        }
                        if message.is_some() {
                            *current_message = message;
                        }
                        return Some(context_compaction_text(&item.content));
                    }
                    None
                });
                if let Some(event_text) = event_text {
                    self.push_right_event(event_text);
                }
            }
            TalosTimelineDelta::Status { .. } => {}
        }
    }

    fn handle_item_completed(
        &mut self,
        item_id: &str,
        status: TalosTimelineItemStatus,
        completed_at_ms: u64,
        final_content: Option<TalosTimelineContent>,
    ) {
        if let Some(item) = self.timeline_items.get_mut(item_id) {
            item.status = status;
            item.completed_at_ms = Some(completed_at_ms);
            if let Some(content) = final_content.clone() {
                item.content = content;
            }
        }
        if let Some(content @ TalosTimelineContent::ContextCompaction { .. }) = &final_content {
            self.push_right_event(context_compaction_text(content));
            return;
        }
        if let Some(TalosTimelineContent::Text { text }) = &final_content {
            self.render_completed_final_text(text);
        }
        let Some(TalosTimelineContent::ToolCall {
            call_id,
            name,
            result: Some(result),
            ..
        }) = final_content
        else {
            return;
        };
        self.push_right_event(format!(
            "tool completed: {} ({})",
            call_id,
            crate::utils::display_tool_name(&name)
        ));
        let _ = result;
    }

    fn handle_assistant_text_committed(&mut self, item_id: &str, phase: TalosTimelinePhase) {
        let Some(item) = self.timeline_items.get_mut(item_id) else {
            return;
        };
        let previous_phase = item.phase;
        item.phase = phase;
        if !matches!(
            (previous_phase, phase),
            (TalosTimelinePhase::Unknown, TalosTimelinePhase::FinalAnswer)
                | (
                    TalosTimelinePhase::FinalAnswer,
                    TalosTimelinePhase::FinalAnswer
                )
        ) {
            return;
        }
        let TalosTimelineContent::Text { text } = &item.content else {
            return;
        };
        if text.trim().is_empty() {
            return;
        }
        self.received_output_delta = true;
    }

    fn render_completed_final_text(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        self.received_output_delta = true;
    }

    fn handle_text_delta(&mut self, item: &TalosTimelineItem, text: &str) {
        if text.is_empty() {
            return;
        }
        match item.phase {
            TalosTimelinePhase::Thinking
            | TalosTimelinePhase::Unknown
            | TalosTimelinePhase::Commentary => {}
            TalosTimelinePhase::FinalAnswer => {
                self.received_output_delta = true;
            }
            TalosTimelinePhase::System => {
                self.push_right_event(text.trim().to_string());
            }
            TalosTimelinePhase::Input | TalosTimelinePhase::Acting => {}
        }
    }

    fn update_tool_arguments(
        &mut self,
        item_id: &str,
        arguments_delta: &str,
    ) -> Option<(String, String, String)> {
        let item = self.timeline_items.get_mut(item_id)?;
        let TalosTimelineContent::ToolCall {
            call_id,
            name,
            arguments,
            ..
        } = &mut item.content
        else {
            return None;
        };
        arguments.push_str(arguments_delta);
        Some((call_id.clone(), name.clone(), arguments.clone()))
    }

    fn update_stored_tool_result(
        &mut self,
        item_id: &str,
        content: Vec<talos_sdk::TalosContentItem>,
        is_error: bool,
    ) {
        if let Some(item) = self.timeline_items.get_mut(item_id) {
            if let TalosTimelineContent::ToolCall { result, .. } = &mut item.content {
                *result = Some(talos_sdk::TalosToolResult { content, is_error });
            }
        }
    }
}

fn context_compaction_text(content: &TalosTimelineContent) -> String {
    let TalosTimelineContent::ContextCompaction {
        stage,
        outcome,
        message,
        ..
    } = content
    else {
        return "context compaction".to_string();
    };
    if let Some(message) = message.as_ref().filter(|value| !value.trim().is_empty()) {
        return message.trim().to_string();
    }
    let outcome = outcome
        .map(|value| format!(" ({})", context_compaction_outcome_label(value)))
        .unwrap_or_default();
    format!(
        "context compaction: {}{}",
        context_compaction_stage_label(*stage),
        outcome
    )
}

pub(crate) fn context_compaction_stage_label(stage: ContextCompactionStage) -> &'static str {
    match stage {
        ContextCompactionStage::Preparing => "preparing",
        ContextCompactionStage::Summarizing => "summarizing",
        ContextCompactionStage::RebuildingPrompt => "rebuilding prompt",
        ContextCompactionStage::Trimming => "trimming",
        ContextCompactionStage::Completed => "completed",
        ContextCompactionStage::Failed => "failed",
    }
}

fn context_compaction_outcome_label(outcome: ContextCompactionOutcome) -> &'static str {
    match outcome {
        ContextCompactionOutcome::Applied => "applied",
        ContextCompactionOutcome::NoChanges => "no changes",
        ContextCompactionOutcome::RejectedMissingSummary => "missing summary",
        ContextCompactionOutcome::TrimmedFallbackApplied => "trimmed fallback applied",
        ContextCompactionOutcome::FailedContextStillTooLarge => "context still too large",
        ContextCompactionOutcome::FailedProviderError => "provider error",
        ContextCompactionOutcome::FailedInternalError => "internal error",
    }
}
