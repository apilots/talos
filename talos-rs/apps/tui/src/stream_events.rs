use std::time::Instant;

use talos_sdk::{
    AgentAssistantTextPhase, AgentEventPayload, AgentMessageBlockContent,
    AgentMessageBlockDeltaContent, AgentMessageRole, AgentPendingInput, AgentRunResult,
    AgentUserContent, ContextCompactionOutcome, ContextCompactionStage, RecoverTimelineResponse,
    SessionHistoryResponse, TimelineEvent, TimelineEventPayload, TimelineSystemNotice,
};
use tokio::sync::mpsc;
use tracing::error;

use crate::render_runtime::render::history::agent_message_to_blocks;
use crate::render_runtime::transcript::block::TranscriptBlockKind;
use crate::TuiApp;

impl TuiApp {
    pub(crate) fn handle_stream_event(&mut self, envelope: TimelineEvent) {
        self.trace_event_count = self.trace_event_count.saturating_add(1);
        let identity = envelope.event.identity();

        if let Some(session_id) = self.session_id.as_deref() {
            if identity.session_id != session_id {
                self.push_right_event(format!(
                    "ignored event from other session: {}",
                    identity.session_id
                ));
                return;
            }
        }

        let event = match envelope.event {
            TimelineEventPayload::SystemNotice(notice) => {
                self.apply_system_notice(envelope.event_seq, notice);
                return;
            }
            TimelineEventPayload::Agent(event) => event,
        };
        let identity = &event.identity;
        match self.active_stream_turn_id.as_deref() {
            Some(active_id) if identity.turn_id != active_id => {
                self.push_right_event(format!(
                    "ignored stale event for {} (active {})",
                    identity.turn_id, active_id
                ));
                return;
            }
            None => {
                self.active_stream_turn_id = Some(identity.turn_id.clone());
            }
            _ => {}
        }

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
                self.context_compaction_stage = None;
            }
            AgentEventPayload::MessageStarted(started) => {
                if started.message.role == AgentMessageRole::User {
                    self.pending_user_action = None;
                }
            }
            AgentEventPayload::MessageBlockStarted(started) => {
                self.observe_started_block(&started.block.content, &started.block.block_id);
            }
            AgentEventPayload::MessageBlockDelta(delta) => {
                self.observe_block_delta(&delta.block_id, delta.delta);
            }
            AgentEventPayload::MessageBlockCompleted(completed) => {
                self.observe_completed_block(&completed.block.content);
            }
            AgentEventPayload::MessageCompleted(_) => {}
            AgentEventPayload::WaitingForInput(pending) => {
                if let AgentPendingInput::UserAction { action } = pending {
                    self.pending_user_action = Some(crate::user_action::PendingActionState::new(
                        event.identity.turn_id.clone(),
                        action,
                    ));
                    self.in_flight = true;
                }
            }
            AgentEventPayload::MetricsUpdated(metrics) => {
                self.turn_metrics = Some(metrics);
                self.turn_metrics_live = true;
            }
            AgentEventPayload::TurnCompleted(_) => {
                if let Some(metrics) = &self.turn_metrics {
                    self.session_usage.accumulate(&metrics.usage);
                }
                self.schedule_session_usage_refresh();
                self.turn_metrics_live = false;
                self.context_compaction_stage = None;
                let user_action_cursor = self
                    .pending_user_action
                    .as_ref()
                    .and(self.last_stream_event_id.clone());
                self.finish_active_stream("stream completed", false, false);
                self.last_stream_event_id = user_action_cursor;
            }
            AgentEventPayload::TurnFailed(_) => {
                self.turn_metrics_live = false;
                self.context_compaction_stage = None;
                self.finish_active_stream("stream error", true, false);
            }
        }
    }

    pub(crate) fn replay_recovery_snapshot(&mut self, response: RecoverTimelineResponse) -> usize {
        let active_turn = response.active_turn.clone();
        let mut events = response.replay.events;
        events.retain(|event| match &event.event {
            TimelineEventPayload::SystemNotice(_) => true,
            TimelineEventPayload::Agent(event) => active_turn
                .as_ref()
                .is_some_and(|active| event.identity.turn_id == active.identity.turn_id),
        });
        events.sort_by_key(|a| a.event_seq);
        let count = events.len();
        for envelope in events {
            if matches!(envelope.event, TimelineEventPayload::Agent(_))
                && self.active_stream_turn_id.as_deref()
                    != Some(envelope.event.identity().turn_id.as_str())
            {
                self.active_stream_turn_id = None;
                self.last_stream_sequence = None;
                self.last_stream_event_id = None;
                self.context_compaction_stage = None;
            }
            self.handle_stream_event(envelope);
        }
        let should_reconnect = active_turn.is_some();
        if should_reconnect {
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
            self.active_stream_turn_id = None;
            self.last_stream_sequence = None;
            self.last_stream_event_id = None;
            self.context_compaction_stage = None;
        }
        count
    }

    pub(crate) fn replay_history_snapshot(&mut self, response: SessionHistoryResponse) -> usize {
        let mut item_count = 0;
        self.active_stream_turn_id = None;
        self.last_stream_sequence = None;
        self.last_stream_event_id = None;
        self.context_compaction_stage = None;

        for turn in response.data {
            let failure = match &turn.result {
                AgentRunResult::Completed(_) => None,
                AgentRunResult::Failed(failure) => Some(failure.failure.error.message.clone()),
            };
            let (messages, round_duration_ms) = match turn.result {
                AgentRunResult::Completed(output) => (output.messages, output.metrics.duration_ms),
                AgentRunResult::Failed(failure) => (failure.messages, failure.metrics.duration_ms),
            };
            for message in messages {
                if message.role == AgentMessageRole::User {
                    for block in &message.blocks {
                        if let AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                            text,
                        }) = &block.content
                        {
                            self.input_history.push(text.clone());
                        }
                    }
                }
                let blocks = agent_message_to_blocks(&message)
                    .into_iter()
                    .map(|block| {
                        if block.kind == TranscriptBlockKind::UserInput {
                            return match round_duration_ms {
                                Some(duration) => block.with_round_duration_ms(duration),
                                None => block,
                            };
                        }
                        block
                    })
                    .collect::<Vec<_>>();
                item_count += self.render_runtime.append_history(blocks);
            }
            if let Some(message) = failure {
                self.push_system_notice(format!("Error: {message}"));
                item_count += 1;
            }
            for envelope in turn.notices {
                if let TimelineEventPayload::SystemNotice(notice) = envelope.event {
                    self.apply_system_notice(envelope.event_seq, notice);
                }
            }
        }

        self.context_compaction_stage = None;
        item_count
    }

    pub(crate) fn apply_system_notice(&mut self, event_seq: i64, notice: TimelineSystemNotice) {
        if self.session_id.as_deref() != Some(notice.identity.session_id.as_str())
            || self
                .render_runtime
                .store()
                .blocks()
                .iter()
                .any(|block| block.timeline_event_seq == Some(event_seq))
        {
            return;
        }
        let level = match notice.notice.level {
            talos_sdk::TalosTimelineNoticeLevel::Info => "Info",
            talos_sdk::TalosTimelineNoticeLevel::Warning => "Warning",
            talos_sdk::TalosTimelineNoticeLevel::Error => "Error",
        };
        // Replay identity belongs to the rendered block, not a separate eviction-prone cache.
        let text = format!("{level}: {}", notice.notice.message());
        let mut block = crate::render_runtime::transcript::block::FinalizedTranscriptBlock::new(
            TranscriptBlockKind::SystemNotice,
            text.lines()
                .map(crate::render_runtime::common::style::StyledLine::plain)
                .collect(),
        );
        block.timeline_event_seq = Some(event_seq);
        self.render_runtime.append_history([block]);
        self.request_chat_activity();
    }

    fn observe_started_block(&mut self, content: &AgentMessageBlockContent, block_id: &str) {
        match content {
            AgentMessageBlockContent::AssistantText(assistant)
                if assistant.phase == AgentAssistantTextPhase::FinalAnswer
                    && !assistant.text.is_empty() =>
            {
                self.received_output_delta = true;
            }
            AgentMessageBlockContent::ToolCall(tool) => {
                self.push_right_event(format!("tool started [{block_id}]: {}", tool.name));
            }
            AgentMessageBlockContent::ExecutionSummary(summary)
                if !summary.text.trim().is_empty() =>
            {
                self.push_right_event(format!("execution summary: {}", summary.text.trim()));
            }
            AgentMessageBlockContent::ContextCompaction(compaction) => {
                self.context_compaction_stage = Some(compaction.stage);
                self.push_right_event(context_compaction_status(
                    compaction.stage,
                    compaction.outcome,
                    compaction.message.as_deref(),
                ));
            }
            AgentMessageBlockContent::SystemNotice(notice)
                if !notice.message().trim().is_empty() =>
            {
                self.push_right_event(notice.message().trim().to_string());
            }
            _ => {}
        }
    }

    fn observe_block_delta(&mut self, block_id: &str, delta: AgentMessageBlockDeltaContent) {
        match delta {
            AgentMessageBlockDeltaContent::Text(text)
                if !text.is_empty()
                    && self.render_runtime.assistant_text_phase(block_id)
                        == Some(AgentAssistantTextPhase::FinalAnswer) =>
            {
                self.received_output_delta = true;
            }
            AgentMessageBlockDeltaContent::AssistantTextPhase(
                AgentAssistantTextPhase::FinalAnswer,
            ) => {
                self.received_output_delta = true;
            }
            AgentMessageBlockDeltaContent::ContextCompaction(delta) => {
                if let Some(stage) = delta.stage {
                    self.context_compaction_stage = Some(stage);
                    self.push_right_event(context_compaction_status(
                        stage,
                        delta.outcome,
                        delta.message.as_deref(),
                    ));
                }
            }
            AgentMessageBlockDeltaContent::ToolCallArguments(_)
            | AgentMessageBlockDeltaContent::ToolProgress(_)
            | AgentMessageBlockDeltaContent::AssistantTextPhase(_)
            | AgentMessageBlockDeltaContent::Text(_) => {}
        }
    }

    fn observe_completed_block(&mut self, content: &AgentMessageBlockContent) {
        match content {
            AgentMessageBlockContent::AssistantText(assistant)
                if assistant.phase == AgentAssistantTextPhase::FinalAnswer
                    && !assistant.text.trim().is_empty() =>
            {
                self.received_output_delta = true;
            }
            AgentMessageBlockContent::ToolResult(result) => {
                self.push_right_event(format!(
                    "tool completed: {} ({})",
                    result.call_id,
                    crate::utils::display_tool_name(&result.tool_name)
                ));
            }
            AgentMessageBlockContent::ExecutionSummary(summary)
                if !summary.text.trim().is_empty() =>
            {
                self.push_right_event(format!("execution summary: {}", summary.text.trim()));
            }
            AgentMessageBlockContent::ContextCompaction(compaction) => {
                self.context_compaction_stage = None;
                self.push_right_event(context_compaction_status(
                    compaction.stage,
                    compaction.outcome,
                    compaction.message.as_deref(),
                ));
            }
            _ => {}
        }
    }
}

fn context_compaction_status(
    stage: ContextCompactionStage,
    outcome: Option<ContextCompactionOutcome>,
    message: Option<&str>,
) -> String {
    if let Some(message) = message.filter(|value| !value.trim().is_empty()) {
        return message.trim().to_string();
    }
    let outcome = outcome
        .map(|value| format!(" ({})", context_compaction_outcome_label(value)))
        .unwrap_or_default();
    format!(
        "context compaction: {}{}",
        context_compaction_stage_label(stage),
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
