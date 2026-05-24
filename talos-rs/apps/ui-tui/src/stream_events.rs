use talos_sdk::{
    SessionHistorySnapshot, TalosContentItem, TalosJsonValue, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineEventPayload, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressKind, TimelineEventEnvelope,
    TimelineRecoverySnapshot,
};

use crate::types::{ChatRole, ToolCallStatus};
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

        match event.payload {
            TalosTimelineEventPayload::TurnStarted(_) => {}
            TalosTimelineEventPayload::ItemStarted(payload) => {
                self.handle_item_started(payload.item);
            }
            TalosTimelineEventPayload::ItemDelta(payload) => {
                self.handle_item_delta(&payload.item_id, payload.delta);
            }
            TalosTimelineEventPayload::ItemCompleted(payload) => {
                self.handle_item_completed(
                    &payload.item_id,
                    payload.status,
                    payload.completed_at_ms,
                    payload.final_content,
                );
            }
            TalosTimelineEventPayload::AssistantTextCommitted(payload) => {
                self.handle_assistant_text_committed(&payload.item_id, payload.phase);
            }
            TalosTimelineEventPayload::TurnCompleted(payload) => {
                self.session_usage.accumulate(&payload.usage);
                self.finish_active_stream("stream completed", false, false);
            }
            TalosTimelineEventPayload::TurnError(payload) => {
                self.push_block(ChatRole::System, format!("Stream error: {}", payload.error));
                self.finish_active_stream("stream error", true, false);
            }
        }
    }

    pub(crate) fn replay_recovery_snapshot(&mut self, snapshot: TimelineRecoverySnapshot) -> usize {
        let mut events = snapshot.replay.events;
        events.sort_by(|a, b| a.event_seq.cmp(&b.event_seq));
        let count = events.len();
        for envelope in events {
            if self.active_stream_message_id.as_deref() != Some(envelope.message_id.as_str()) {
                self.active_stream_message_id = None;
                self.last_stream_sequence = None;
                self.timeline_items.clear();
            }
            self.handle_stream_event(envelope);
        }
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.timeline_items.clear();
        count
    }

    pub(crate) fn replay_history_snapshot(&mut self, snapshot: SessionHistorySnapshot) -> usize {
        let mut item_count = 0;
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
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
                self.transcript.note_visible_activity();
                self.append_thinking(text);
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
                self.transcript.note_visible_activity();
                self.append_assistant(text);
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
                self.transcript.note_visible_activity();
                self.append_thinking(text);
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
                self.transcript.note_visible_activity();
                self.finish_live_text_blocks();
                let timing_ms = item
                    .completed_at_ms
                    .map(|completed| completed.saturating_sub(item.started_at_ms));
                self.push_tool_call_block(
                    call_id.clone(),
                    name.clone(),
                    parse_tool_arguments(arguments),
                    0,
                    item.started_at_ms,
                    timing_ms,
                );
                for entry in progress {
                    self.append_tool_call_progress(
                        call_id,
                        Self::format_tool_progress_line(entry.progress_kind, &entry.text),
                    );
                }
                if let Some(result) = result {
                    self.complete_tool_call_block(
                        call_id,
                        Self::tool_result_output_text(&result.content),
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
                    self.transcript.note_visible_activity();
                    self.append_thinking(text);
                }
            }
            (
                TalosTimelineItemKind::AssistantText,
                TalosTimelinePhase::Unknown | TalosTimelinePhase::Commentary,
                TalosTimelineContent::Text { text },
            ) => {
                if !text.is_empty() {
                    self.transcript.note_visible_activity();
                    self.append_thinking(text);
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
                self.transcript.note_visible_activity();
                self.finish_live_text_blocks();
                self.push_right_event(format!("tool started [{}]: {}", item.item_id, name));
                self.upsert_tool_event(call_id.clone(), format!("queued: {}", name));
                self.push_tool_call_block(
                    call_id.clone(),
                    name.clone(),
                    parse_tool_arguments(arguments),
                    0,
                    item.started_at_ms,
                    item.completed_at_ms
                        .map(|completed| completed.saturating_sub(item.started_at_ms)),
                );
                self.update_tool_call_status(call_id, ToolCallStatus::Queued);
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
                TalosTimelineItemKind::SystemNotice,
                TalosTimelinePhase::System,
                TalosTimelineContent::SystemNotice { message, .. },
            ) => {
                if !message.trim().is_empty() {
                    self.push_right_event(message.trim().to_string());
                }
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
                self.push_tool_call_block(
                    call_id,
                    name,
                    parse_tool_arguments(&arguments),
                    0,
                    0,
                    None,
                );
            }
            TalosTimelineDelta::ToolProgress {
                progress_kind,
                text_delta,
            } => {
                if let Some(call_id) = self.tool_call_id(item_id) {
                    self.transcript.note_visible_activity();
                    self.append_tool_call_progress(
                        &call_id,
                        Self::format_tool_progress_line(progress_kind, &text_delta),
                    );
                }
            }
            TalosTimelineDelta::ToolResult { content, is_error } => {
                if let Some(call_id) = self.tool_call_id(item_id) {
                    let output = Self::tool_result_output_text(&content);
                    self.complete_tool_call_block(&call_id, output, !is_error, None);
                    self.clear_live_tool_progress(&call_id);
                    self.update_stored_tool_result(item_id, content, is_error);
                }
            }
            TalosTimelineDelta::ToolDynamicRequest {
                call_id,
                tool_name,
                arguments,
            } => {
                self.transcript.note_visible_activity();
                self.push_right_event(format!(
                    "dynamic tool requested: {} ({})",
                    crate::utils::display_tool_name(&tool_name),
                    call_id
                ));
                self.upsert_tool_event(
                    call_id.clone(),
                    format!("running: {}", crate::utils::display_tool_name(&tool_name)),
                );
                self.push_tool_call_block(call_id.clone(), tool_name, arguments, 0, 0, None);
                self.update_tool_call_status(&call_id, ToolCallStatus::PendingDynamicInput);
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
        self.upsert_tool_event(
            call_id.clone(),
            format!("done: {}", crate::utils::display_tool_name(&name)),
        );
        self.complete_tool_call_block(
            &call_id,
            Self::tool_result_output_text(&result.content),
            !result.is_error,
            None,
        );
        self.clear_live_tool_progress(&call_id);
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
        ) {
            return;
        }
        let TalosTimelineContent::Text { text } = &item.content else {
            return;
        };
        let text = text.clone();
        if text.trim().is_empty() {
            return;
        }
        self.remove_text_from_live_thinking(&text);
        self.transcript.note_visible_activity();
        self.append_assistant(&text);
        self.received_output_delta = true;
    }

    fn remove_text_from_live_thinking(&mut self, text: &str) {
        let Some(idx) = self.transcript.live_thinking_idx else {
            return;
        };
        let Some(block) = self.chat_blocks.get_mut(idx) else {
            self.transcript.live_thinking_idx = None;
            return;
        };
        if block.content == text {
            self.remove_chat_block(idx);
            self.rebuild_chat_history();
            self.transcript.live_thinking_idx = None;
            return;
        }
        if let Some(start) = block.content.rfind(text) {
            let end = start + text.len();
            block.content.replace_range(start..end, "");
        }
        let should_remove = self
            .chat_blocks
            .get(idx)
            .map(|block| block.content.trim().is_empty())
            .unwrap_or(false);
        if should_remove {
            self.remove_chat_block(idx);
            self.rebuild_chat_history();
            self.transcript.live_thinking_idx = None;
        } else {
            self.sync_history_entry(idx);
        }
    }

    fn handle_text_delta(&mut self, item: &TalosTimelineItem, text: &str) {
        if text.is_empty() {
            return;
        }
        match item.phase {
            TalosTimelinePhase::Thinking => {
                self.transcript.note_visible_activity();
                self.append_thinking(text);
            }
            TalosTimelinePhase::Unknown => {
                self.transcript.note_visible_activity();
                self.append_thinking(text);
            }
            TalosTimelinePhase::Commentary => {
                self.transcript.note_visible_activity();
                self.append_thinking(text);
            }
            TalosTimelinePhase::FinalAnswer => {
                self.transcript.note_visible_activity();
                self.append_assistant(text);
                self.received_output_delta = true;
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

fn parse_tool_arguments(arguments: &str) -> TalosJsonValue {
    serde_json::from_str::<serde_json::Value>(arguments)
        .map(TalosJsonValue::from)
        .unwrap_or_else(|_| TalosJsonValue::from(serde_json::Value::String(arguments.to_string())))
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
