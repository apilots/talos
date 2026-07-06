use talos_sdk::{
    AgentEventPayload, ContextCompactionOutcome, ContextCompactionStage, SessionHistorySnapshot,
    TalosContentItem, TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem,
    TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressKind,
    TimelineEventEnvelope, TimelineRecoveryResponse, ToolArguments,
};

use crate::types::{ChatRole, ToolCallBlockUpdate, ToolCallStatus};
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
        self.request_follow_bottom();

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
                self.push_block(ChatRole::System, format!("Stream error: {}", payload.error));
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
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.last_stream_event_id = None;
        self.timeline_items.clear();
        count
    }

    pub(crate) fn replay_history_snapshot(&mut self, snapshot: SessionHistorySnapshot) -> usize {
        let mut item_count = 0;
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.last_stream_event_id = None;
        self.timeline_items.clear();
        self.finish_live_text_blocks();

        for message in snapshot.messages {
            self.finish_live_text_blocks();
            for item in message.items {
                if self.render_history_item(item) {
                    item_count += 1;
                }
            }
        }

        self.finish_live_text_blocks();
        self.timeline_items.clear();
        item_count
    }

    fn render_history_item(&mut self, item: TalosTimelineItem) -> bool {
        match (&item.kind, &item.phase, &item.content) {
            (
                TalosTimelineItemKind::UserInput,
                TalosTimelinePhase::Input,
                TalosTimelineContent::Text { text },
            ) => {
                if text.trim().is_empty() {
                    return false;
                }
                self.push_block(ChatRole::User, text.clone());
                true
            }
            (
                TalosTimelineItemKind::Reasoning,
                TalosTimelinePhase::Thinking,
                TalosTimelineContent::Text { text },
            ) => {
                if text.trim().is_empty() {
                    return false;
                }
                self.append_thinking(None, text);
                true
            }
            (
                TalosTimelineItemKind::AssistantText,
                TalosTimelinePhase::FinalAnswer,
                TalosTimelineContent::Text { text },
            ) => {
                if text.trim().is_empty() {
                    return false;
                }
                self.append_final_answer(None, text);
                true
            }
            (
                TalosTimelineItemKind::AssistantText,
                TalosTimelinePhase::Unknown | TalosTimelinePhase::Commentary,
                TalosTimelineContent::Text { text },
            ) => {
                if text.trim().is_empty() {
                    return false;
                }
                self.append_commentary(None, text);
                true
            }
            (
                TalosTimelineItemKind::ToolCall,
                TalosTimelinePhase::Acting,
                TalosTimelineContent::ToolCall {
                    call_id,
                    name,
                    arguments,
                    progress,
                    result,
                },
            ) => {
                self.finish_live_text_blocks();
                let timing_ms = item
                    .completed_at_ms
                    .map(|completed| completed.saturating_sub(item.started_at_ms));
                if !is_internal_task_output(name) {
                    self.push_tool_call_block(ToolCallBlockUpdate {
                        item_id: Some(item.item_id.clone()),
                        call_id: call_id.clone(),
                        tool_name: name.clone(),
                        tool_input: parse_tool_arguments(arguments),
                        iteration: 0,
                        item_index: item.started_at_ms,
                        timing_ms,
                    });
                }
                for entry in progress {
                    self.append_tool_call_progress(
                        call_id,
                        Self::format_tool_progress_line(entry.progress_kind, &entry.text),
                    );
                }
                if let Some(result) = result {
                    let output = Self::tool_result_output_text(&result.content);
                    self.complete_tool_call_block(
                        Some(&item.item_id),
                        call_id,
                        output,
                        result.content.clone(),
                        !result.is_error,
                        timing_ms,
                    );
                } else {
                    self.update_tool_call_status(call_id, timeline_tool_status(item.status));
                }
                true
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
                if message.trim().is_empty() {
                    return false;
                }
                self.push_right_event(message.trim().to_string());
                true
            }
            _ => false,
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
                if !text.is_empty() && self.active_user_idx.is_none() {
                    self.push_block(ChatRole::User, text.clone());
                }
            }
            (
                TalosTimelineItemKind::Reasoning,
                TalosTimelinePhase::Thinking,
                TalosTimelineContent::Text { text },
            ) => {
                if !text.is_empty() {
                    self.append_thinking(Some(&item.item_id), text);
                }
            }
            (
                TalosTimelineItemKind::AssistantText,
                TalosTimelinePhase::Unknown | TalosTimelinePhase::Commentary,
                TalosTimelineContent::Text { text },
            ) => {
                if !text.is_empty() {
                    self.append_commentary(Some(&item.item_id), text);
                }
            }
            (
                TalosTimelineItemKind::AssistantText,
                TalosTimelinePhase::FinalAnswer,
                TalosTimelineContent::Text { text },
            ) => {
                if !text.is_empty() {
                    self.buffer_final_answer(&item.item_id, text);
                }
            }
            (
                TalosTimelineItemKind::ToolCall,
                TalosTimelinePhase::Acting,
                TalosTimelineContent::ToolCall {
                    call_id,
                    name,
                    arguments,
                    ..
                },
            ) => {
                if is_internal_task_output(name) {
                    return;
                }
                self.finish_live_text_blocks();
                self.push_right_event(format!("tool started [{}]: {}", item.item_id, name));
                self.push_tool_call_block(ToolCallBlockUpdate {
                    item_id: Some(item.item_id.clone()),
                    call_id: call_id.clone(),
                    tool_name: name.clone(),
                    tool_input: parse_tool_arguments(arguments),
                    iteration: 0,
                    item_index: item.started_at_ms,
                    timing_ms: item
                        .completed_at_ms
                        .map(|completed| completed.saturating_sub(item.started_at_ms)),
                });
                self.update_tool_call_status(call_id, ToolCallStatus::Running);
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
                let Some((call_id, name, arguments)) =
                    self.update_tool_arguments(item_id, &arguments_delta)
                else {
                    return;
                };
                if is_internal_task_output(&name) {
                    return;
                }
                self.push_tool_call_block(ToolCallBlockUpdate {
                    item_id: Some(item_id.to_string()),
                    call_id,
                    tool_name: name,
                    tool_input: parse_tool_arguments(&arguments),
                    iteration: 0,
                    item_index: 0,
                    timing_ms: None,
                });
            }
            TalosTimelineDelta::ToolProgress {
                progress_kind,
                text_delta,
            } => {
                if let Some(call_id) = self.tool_call_id(item_id) {
                    self.append_tool_call_progress(
                        &call_id,
                        Self::format_tool_progress_line(progress_kind, &text_delta),
                    );
                }
            }
            TalosTimelineDelta::ToolResult { content, is_error } => {
                if let Some(call_id) = self.tool_call_id(item_id) {
                    let output = Self::tool_result_output_text(&content);
                    self.complete_tool_call_block(
                        Some(item_id),
                        &call_id,
                        output,
                        content.clone(),
                        !is_error,
                        None,
                    );
                    self.update_stored_tool_result(item_id, content, is_error);
                }
            }
            TalosTimelineDelta::ToolDynamicRequest {
                call_id,
                tool_name,
                arguments,
            } => {
                self.push_right_event(format!(
                    "dynamic tool requested: {} ({})",
                    crate::utils::display_tool_name(&tool_name),
                    call_id
                ));
                self.push_tool_call_block(ToolCallBlockUpdate {
                    item_id: Some(item_id.to_string()),
                    call_id: call_id.clone(),
                    tool_name,
                    tool_input: ToolArguments::from(serde_json::Value::from(arguments)),
                    iteration: 0,
                    item_index: 0,
                    timing_ms: None,
                });
                self.update_tool_call_status(&call_id, ToolCallStatus::Running);
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
            TalosTimelineDelta::Status { status } => {
                if let Some(call_id) = self.tool_call_id(item_id) {
                    self.update_tool_call_status(&call_id, timeline_tool_status(status));
                }
            }
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
            self.render_completed_final_text(item_id, text);
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
        self.complete_tool_call_block(
            Some(item_id),
            &call_id,
            Self::tool_result_output_text(&result.content),
            result.content.clone(),
            !result.is_error,
            None,
        );
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
            if phase == TalosTimelinePhase::Commentary {
                let _ = self.move_live_text_block(item_id, ChatRole::Commentary);
            }
            return;
        }
        let TalosTimelineContent::Text { text } = &item.content else {
            return;
        };
        let text = text.clone();
        if text.trim().is_empty() {
            return;
        }
        let _ = self.move_live_text_block(item_id, ChatRole::FinalAnswer);
        self.open_final_answer(item_id);
        self.received_output_delta = true;
    }

    fn render_completed_final_text(&mut self, item_id: &str, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if self
            .transcript
            .pending_final_answer_text
            .contains_key(item_id)
        {
            self.open_final_answer(item_id);
            self.complete_final_answer_text(item_id, text);
            return;
        }
        if self.transcript.live_text_blocks.contains_key(item_id) {
            self.open_final_answer(item_id);
            self.complete_final_answer_text(item_id, text);
            return;
        }
        if self.final_answer_text_already_rendered(text) {
            return;
        }
        self.open_final_answer(item_id);
        self.append_final_answer(Some(item_id), text);
        self.received_output_delta = true;
    }

    fn complete_final_answer_text(&mut self, item_id: &str, text: &str) {
        let Some(idx) = self.transcript.live_text_blocks.get(item_id).copied() else {
            return;
        };
        let Some(existing) = self.chat_blocks.get(idx).map(|block| block.content.clone()) else {
            return;
        };
        if existing == text {
            return;
        }
        if let Some(suffix) = text.strip_prefix(&existing) {
            self.append_final_answer(Some(item_id), suffix);
            return;
        }
        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content = text.to_string();
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    fn final_answer_text_already_rendered(&self, text: &str) -> bool {
        self.chat_blocks
            .iter()
            .any(|block| block.role == ChatRole::FinalAnswer && block.content.contains(text))
    }

    fn handle_text_delta(&mut self, item: &TalosTimelineItem, text: &str) {
        if text.is_empty() {
            return;
        }
        match item.phase {
            TalosTimelinePhase::Thinking => {
                self.append_thinking(Some(&item.item_id), text);
            }
            TalosTimelinePhase::Unknown => {
                self.append_commentary(Some(&item.item_id), text);
            }
            TalosTimelinePhase::Commentary => {
                self.append_commentary(Some(&item.item_id), text);
            }
            TalosTimelinePhase::FinalAnswer => {
                if self.final_answer_is_open(&item.item_id) {
                    self.append_final_answer(Some(&item.item_id), text);
                    self.received_output_delta = true;
                } else {
                    self.buffer_final_answer(&item.item_id, text);
                }
            }
            TalosTimelinePhase::System => {
                self.push_right_event(text.trim().to_string());
            }
            TalosTimelinePhase::Input | TalosTimelinePhase::Acting => {}
        }
    }

    fn tool_call_id(&self, item_id: &str) -> Option<String> {
        match self.timeline_items.get(item_id).map(|item| &item.content) {
            Some(TalosTimelineContent::ToolCall { call_id, .. }) => Some(call_id.clone()),
            _ => None,
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
        content: Vec<TalosContentItem>,
        is_error: bool,
    ) {
        if let Some(item) = self.timeline_items.get_mut(item_id) {
            if let TalosTimelineContent::ToolCall { result, .. } = &mut item.content {
                *result = Some(talos_sdk::TalosToolResult { content, is_error });
            }
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

fn parse_tool_arguments(arguments: &str) -> ToolArguments {
    serde_json::from_str::<serde_json::Value>(arguments)
        .map(ToolArguments::from)
        .unwrap_or_else(|_| ToolArguments::from(serde_json::Value::String(arguments.to_string())))
}

fn is_internal_task_output(tool_name: &str) -> bool {
    tool_name.eq_ignore_ascii_case("TaskOutput")
}

fn timeline_tool_status(status: TalosTimelineItemStatus) -> ToolCallStatus {
    match status {
        TalosTimelineItemStatus::Running => ToolCallStatus::Running,
        TalosTimelineItemStatus::WaitingDynamicInput => ToolCallStatus::PendingDynamicInput,
        TalosTimelineItemStatus::Completed => ToolCallStatus::Completed { exit_ok: true },
        TalosTimelineItemStatus::Failed => ToolCallStatus::Completed { exit_ok: false },
        TalosTimelineItemStatus::Cancelled => ToolCallStatus::Completed { exit_ok: false },
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

fn context_compaction_stage_label(stage: ContextCompactionStage) -> &'static str {
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
        ContextCompactionOutcome::RejectedMissingSummary => "missing summary",
        ContextCompactionOutcome::TrimmedFallbackApplied => "trimmed fallback applied",
        ContextCompactionOutcome::FailedContextStillTooLarge => "context still too large",
        ContextCompactionOutcome::FailedProviderError => "provider error",
        ContextCompactionOutcome::FailedInternalError => "internal error",
    }
}
