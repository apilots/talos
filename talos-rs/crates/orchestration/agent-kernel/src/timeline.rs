use chrono::Utc;
use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{
    StopReason, TalosAssistantTextCommittedEvent, TalosContentItem, TalosItemCompletedEvent,
    TalosItemDeltaEvent, TalosItemStartedEvent, TalosTimelineContent, TalosTimelineDelta,
    TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineEventType, TalosTimelineItem,
    TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressEntry,
    TalosToolProgressKind, TalosToolResult, TalosTurnCompletedEvent, TalosTurnErrorEvent,
    TalosTurnMetricsEvent, TalosTurnStartedEvent, TalosUsage, TurnExecutionMetrics,
};

#[derive(Debug, Clone)]
struct TimelineItemState {
    item: TalosTimelineItem,
}

#[derive(Debug, Clone)]
pub struct TimelineBuilder {
    request_id: String,
    turn_id: String,
    message_id: String,
    session_id: String,
    next_sequence: u64,
    next_item_index: u64,
    items: HashMap<String, TimelineItemState>,
}

impl TimelineBuilder {
    pub fn new(
        request_id: impl Into<String>,
        turn_id: impl Into<String>,
        message_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            turn_id: turn_id.into(),
            message_id: message_id.into(),
            session_id: session_id.into(),
            next_sequence: 0,
            next_item_index: 0,
            items: HashMap::new(),
        }
    }

    pub fn start_turn(&mut self) -> TalosTimelineEvent {
        let payload = TalosTimelineEventPayload::TurnStarted(TalosTurnStartedEvent {
            request_id: self.request_id.clone(),
            turn_id: self.turn_id.clone(),
            started_at_ms: now_ms(),
        });
        self.event(TalosTimelineEventType::TurnStarted, payload)
    }

    pub fn complete_turn(
        &mut self,
        stop_reason: StopReason,
        usage: TalosUsage,
        metrics: Option<TurnExecutionMetrics>,
    ) -> TalosTimelineEvent {
        let payload = TalosTimelineEventPayload::TurnCompleted(TalosTurnCompletedEvent {
            request_id: self.request_id.clone(),
            turn_id: self.turn_id.clone(),
            completed_at_ms: now_ms(),
            stop_reason,
            usage,
            metrics,
        });
        self.event(TalosTimelineEventType::TurnCompleted, payload)
    }

    pub fn turn_metrics(&mut self, metrics: TurnExecutionMetrics) -> TalosTimelineEvent {
        let payload = TalosTimelineEventPayload::TurnMetrics(TalosTurnMetricsEvent {
            request_id: self.request_id.clone(),
            turn_id: self.turn_id.clone(),
            metrics,
        });
        self.event(TalosTimelineEventType::TurnMetrics, payload)
    }

    pub fn fail_turn(&mut self, error: impl Into<String>) -> TalosTimelineEvent {
        let payload = TalosTimelineEventPayload::TurnError(TalosTurnErrorEvent {
            request_id: self.request_id.clone(),
            turn_id: self.turn_id.clone(),
            error: error.into(),
            failed_at_ms: now_ms(),
        });
        self.event(TalosTimelineEventType::TurnError, payload)
    }

    pub fn start_item(
        &mut self,
        kind: TalosTimelineItemKind,
        phase: TalosTimelinePhase,
        title: Option<String>,
        content: TalosTimelineContent,
    ) -> TalosTimelineEvent {
        let item_id = self.next_item_id(kind);
        let item = TalosTimelineItem {
            item_id: item_id.clone(),
            parent_item_id: None,
            kind,
            phase,
            status: TalosTimelineItemStatus::Running,
            title,
            started_at_ms: now_ms(),
            completed_at_ms: None,
            content,
        };

        self.items
            .insert(item_id, TimelineItemState { item: item.clone() });

        let payload = TalosTimelineEventPayload::ItemStarted(TalosItemStartedEvent {
            request_id: self.request_id.clone(),
            turn_id: self.turn_id.clone(),
            item,
        });
        self.event(TalosTimelineEventType::ItemStarted, payload)
    }

    pub fn start_reasoning(&mut self) -> TalosTimelineEvent {
        self.start_item(
            TalosTimelineItemKind::Reasoning,
            TalosTimelinePhase::Thinking,
            Some("Thinking".to_string()),
            TalosTimelineContent::Text {
                text: String::new(),
            },
        )
    }

    pub fn start_user_input(&mut self, text: impl Into<String>) -> TalosTimelineEvent {
        self.start_item(
            TalosTimelineItemKind::UserInput,
            TalosTimelinePhase::Input,
            None,
            TalosTimelineContent::Text { text: text.into() },
        )
    }

    pub fn start_commentary(&mut self) -> TalosTimelineEvent {
        self.start_item(
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::Commentary,
            None,
            TalosTimelineContent::Text {
                text: String::new(),
            },
        )
    }

    pub fn start_unknown_assistant_text(&mut self) -> TalosTimelineEvent {
        self.start_item(
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::Unknown,
            None,
            TalosTimelineContent::Text {
                text: String::new(),
            },
        )
    }

    pub fn start_tool_call(
        &mut self,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> TalosTimelineEvent {
        self.start_item(
            TalosTimelineItemKind::ToolCall,
            TalosTimelinePhase::Acting,
            None,
            TalosTimelineContent::ToolCall {
                call_id: call_id.into(),
                name: name.into(),
                arguments: arguments.into(),
                progress: Vec::new(),
                result: None,
            },
        )
    }

    pub fn track_existing_tool_call(
        &mut self,
        item_id: impl Into<String>,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) {
        let item_id = item_id.into();
        let item = TalosTimelineItem {
            item_id: item_id.clone(),
            parent_item_id: None,
            kind: TalosTimelineItemKind::ToolCall,
            phase: TalosTimelinePhase::Acting,
            status: TalosTimelineItemStatus::Running,
            title: None,
            started_at_ms: now_ms(),
            completed_at_ms: None,
            content: TalosTimelineContent::ToolCall {
                call_id: call_id.into(),
                name: name.into(),
                arguments: arguments.into(),
                progress: Vec::new(),
                result: None,
            },
        };
        self.items.insert(item_id, TimelineItemState { item });
    }

    pub fn start_final_answer(&mut self) -> TalosTimelineEvent {
        self.start_item(
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::FinalAnswer,
            None,
            TalosTimelineContent::Text {
                text: String::new(),
            },
        )
    }

    pub fn append_delta(
        &mut self,
        item_id: &str,
        delta: TalosTimelineDelta,
    ) -> TalosResult<TalosTimelineEvent> {
        self.apply_delta(item_id, &delta)?;
        let payload = TalosTimelineEventPayload::ItemDelta(TalosItemDeltaEvent {
            request_id: self.request_id.clone(),
            turn_id: self.turn_id.clone(),
            item_id: item_id.to_string(),
            delta,
        });
        Ok(self.event(TalosTimelineEventType::ItemDelta, payload))
    }

    pub fn complete_item(
        &mut self,
        item_id: &str,
        status: TalosTimelineItemStatus,
        final_content: Option<TalosTimelineContent>,
    ) -> TalosResult<TalosTimelineEvent> {
        let completed_at_ms = now_ms();
        let state = self.active_item_mut(item_id)?;
        state.item.status = status;
        state.item.completed_at_ms = Some(completed_at_ms);
        if let Some(content) = final_content.clone() {
            state.item.content = content;
        }

        let payload = TalosTimelineEventPayload::ItemCompleted(TalosItemCompletedEvent {
            request_id: self.request_id.clone(),
            turn_id: self.turn_id.clone(),
            item_id: item_id.to_string(),
            status,
            completed_at_ms,
            final_content,
        });
        Ok(self.event(TalosTimelineEventType::ItemCompleted, payload))
    }

    pub fn commit_assistant_text(
        &mut self,
        item_id: &str,
        phase: TalosTimelinePhase,
    ) -> TalosResult<TalosTimelineEvent> {
        let state = self.items.get_mut(item_id).ok_or_else(|| {
            TalosError::ValidationError(format!("timeline item not found: {item_id}"))
        })?;
        if !matches!(state.item.kind, TalosTimelineItemKind::AssistantText) {
            return Err(TalosError::ValidationError(format!(
                "timeline item is not assistant text: {item_id}"
            )));
        }
        state.item.phase = phase;
        let payload =
            TalosTimelineEventPayload::AssistantTextCommitted(TalosAssistantTextCommittedEvent {
                request_id: self.request_id.clone(),
                turn_id: self.turn_id.clone(),
                item_id: item_id.to_string(),
                phase,
            });
        Ok(self.event(TalosTimelineEventType::AssistantTextCommitted, payload))
    }

    pub fn close_active_items(
        &mut self,
        status: TalosTimelineItemStatus,
    ) -> TalosResult<Vec<TalosTimelineEvent>> {
        let item_ids = self
            .items
            .iter()
            .filter_map(|(item_id, state)| {
                if should_close_item(state.item.status, status) {
                    Some(item_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        item_ids
            .into_iter()
            .map(|item_id| self.complete_item(&item_id, status, None))
            .collect()
    }

    pub fn item(&self, item_id: &str) -> Option<&TalosTimelineItem> {
        self.items.get(item_id).map(|state| &state.item)
    }

    fn apply_delta(&mut self, item_id: &str, delta: &TalosTimelineDelta) -> TalosResult<()> {
        let state = self.active_item_mut(item_id)?;
        match delta {
            TalosTimelineDelta::Text { text } => append_text(&mut state.item.content, text),
            TalosTimelineDelta::ToolArguments { arguments_delta } => {
                append_tool_arguments(&mut state.item.content, arguments_delta)
            }
            TalosTimelineDelta::ToolProgress {
                progress_kind,
                text_delta,
            } => append_tool_progress(&mut state.item.content, *progress_kind, text_delta),
            TalosTimelineDelta::ToolResult { content, is_error } => {
                set_tool_result(&mut state.item.content, content.clone(), *is_error)
            }
            TalosTimelineDelta::ToolDynamicRequest { .. } => {
                state.item.status = TalosTimelineItemStatus::WaitingDynamicInput;
                Ok(())
            }
            TalosTimelineDelta::Status { status } => {
                state.item.status = *status;
                Ok(())
            }
        }
    }

    fn active_item_mut(&mut self, item_id: &str) -> TalosResult<&mut TimelineItemState> {
        let state = self.items.get_mut(item_id).ok_or_else(|| {
            TalosError::ValidationError(format!("timeline item not found: {item_id}"))
        })?;
        if matches!(
            state.item.status,
            TalosTimelineItemStatus::Completed
                | TalosTimelineItemStatus::Failed
                | TalosTimelineItemStatus::Cancelled
        ) {
            return Err(TalosError::ValidationError(format!(
                "timeline item is not active: {item_id}"
            )));
        }
        Ok(state)
    }

    fn next_item_id(&mut self, kind: TalosTimelineItemKind) -> String {
        self.next_item_index += 1;
        let prefix = match kind {
            TalosTimelineItemKind::UserInput => "input",
            TalosTimelineItemKind::Reasoning => "reasoning",
            TalosTimelineItemKind::AssistantText => "assistant",
            TalosTimelineItemKind::ToolCall => "tool",
            TalosTimelineItemKind::ExecutionSummary => "summary",
            TalosTimelineItemKind::SystemNotice => "system",
        };
        format!("{prefix}-{}", self.next_item_index)
    }

    fn event(
        &mut self,
        event_type: TalosTimelineEventType,
        payload: TalosTimelineEventPayload,
    ) -> TalosTimelineEvent {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        TalosTimelineEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type,
            turn_id: self.turn_id.clone(),
            message_id: self.message_id.clone(),
            session_id: self.session_id.clone(),
            sequence,
            payload,
        }
    }
}

fn should_close_item(current: TalosTimelineItemStatus, terminal: TalosTimelineItemStatus) -> bool {
    match current {
        TalosTimelineItemStatus::Running => true,
        TalosTimelineItemStatus::WaitingDynamicInput => {
            terminal != TalosTimelineItemStatus::Completed
        }
        TalosTimelineItemStatus::Completed
        | TalosTimelineItemStatus::Failed
        | TalosTimelineItemStatus::Cancelled => false,
    }
}

fn append_text(content: &mut TalosTimelineContent, text: &str) -> TalosResult<()> {
    match content {
        TalosTimelineContent::Empty => {
            *content = TalosTimelineContent::Text {
                text: text.to_string(),
            };
            Ok(())
        }
        TalosTimelineContent::Text { text: current } => {
            current.push_str(text);
            Ok(())
        }
        _ => Err(TalosError::ValidationError(
            "text delta can only update text timeline content".to_string(),
        )),
    }
}

fn append_tool_arguments(
    content: &mut TalosTimelineContent,
    arguments_delta: &str,
) -> TalosResult<()> {
    match content {
        TalosTimelineContent::ToolCall { arguments, .. } => {
            arguments.push_str(arguments_delta);
            Ok(())
        }
        _ => Err(TalosError::ValidationError(
            "tool arguments delta can only update tool timeline content".to_string(),
        )),
    }
}

fn append_tool_progress(
    content: &mut TalosTimelineContent,
    progress_kind: TalosToolProgressKind,
    text_delta: &str,
) -> TalosResult<()> {
    match content {
        TalosTimelineContent::ToolCall { progress, .. } => {
            progress.push(TalosToolProgressEntry {
                progress_kind,
                text: text_delta.to_string(),
                timestamp_ms: now_ms(),
            });
            Ok(())
        }
        _ => Err(TalosError::ValidationError(
            "tool progress delta can only update tool timeline content".to_string(),
        )),
    }
}

fn set_tool_result(
    content: &mut TalosTimelineContent,
    result_content: Vec<TalosContentItem>,
    is_error: bool,
) -> TalosResult<()> {
    match content {
        TalosTimelineContent::ToolCall { result, .. } => {
            *result = Some(TalosToolResult {
                content: result_content,
                is_error,
            });
            Ok(())
        }
        _ => Err(TalosError::ValidationError(
            "tool result delta can only update tool timeline content".to_string(),
        )),
    }
}

fn now_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
