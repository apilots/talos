use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use talos_error::TalosResult;
use talos_memory::{MemoryStore, TalosTurn};
use talos_session_store::{TimelineEventId, TimelineEventService};
use talos_types::talos::{
    SessionHistoryMessage, SessionHistorySnapshot, StopReason, TalosContentItem, TalosOutputItem,
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineEvent, TalosTimelineEventPayload,
    TalosTimelineItem, TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase,
    TalosToolProgressEntry, TalosToolResult, TalosUsage,
};
use talos_types::TalosConfig;

const STABLE_EVENT_SCAN_BATCH_SIZE: usize = 2048;

#[derive(Clone)]
pub struct SessionHistoryService {
    memory_store: Arc<MemoryStore>,
    event_service: Arc<TimelineEventService>,
}

impl SessionHistoryService {
    pub fn new(base_config: TalosConfig, event_service: Arc<TimelineEventService>) -> Self {
        Self {
            memory_store: Arc::new(MemoryStore::from_config(&base_config.agent.kernel)),
            event_service,
        }
    }

    pub async fn snapshot(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot> {
        let turns = self.memory_store.load_session_turns(session_id).await?;
        let mut messages = map_turns_to_history_messages(turns);
        let completed_timeline_messages = self.completed_timeline_messages(session_id).await?;
        let last_stable_event_id = self
            .last_stable_event_id(session_id, &completed_timeline_messages)
            .await?;
        for timeline_message in completed_timeline_messages {
            merge_completed_timeline_message(&mut messages, timeline_message);
        }
        Ok(SessionHistorySnapshot {
            session_id: session_id.to_string(),
            messages,
            generated_at_ms: chrono::Utc::now().timestamp_millis(),
            last_stable_event_id,
        })
    }

    async fn completed_timeline_messages(
        &self,
        session_id: &str,
    ) -> TalosResult<Vec<SessionHistoryMessage>> {
        let mut cursor: Option<TimelineEventId> = None;
        let mut collected = Vec::new();
        loop {
            let events = self
                .event_service
                .list_events_after(session_id, cursor.as_ref(), STABLE_EVENT_SCAN_BATCH_SIZE)
                .await?;
            if events.is_empty() {
                break;
            }
            collected.extend(events.iter().cloned());
            cursor = events
                .last()
                .and_then(|event| event.event_id.clone())
                .map(TimelineEventId::from);
            if events.len() < STABLE_EVENT_SCAN_BATCH_SIZE {
                break;
            }
        }
        collected.sort_by_key(|event| event.event_seq.unwrap_or_default());
        Ok(history_messages_from_completed_events(collected))
    }

    async fn last_stable_event_id(
        &self,
        session_id: &str,
        messages: &[SessionHistoryMessage],
    ) -> TalosResult<Option<String>> {
        let stable_keys = stable_message_keys(messages);
        if stable_keys.is_empty() {
            return Ok(None);
        }

        let mut cursor: Option<TimelineEventId> = None;
        let mut last_stable_event_id = None;
        loop {
            let events = self
                .event_service
                .list_events_after(session_id, cursor.as_ref(), STABLE_EVENT_SCAN_BATCH_SIZE)
                .await?;
            if events.is_empty() {
                break;
            }
            for event in &events {
                if stable_keys.contains_event(event) {
                    last_stable_event_id = event.event_id.clone();
                }
            }
            cursor = events
                .last()
                .and_then(|event| event.event_id.clone())
                .map(TimelineEventId::from);
            if events.len() < STABLE_EVENT_SCAN_BATCH_SIZE {
                break;
            }
        }
        Ok(last_stable_event_id)
    }
}

struct StableMessageKeys<'a> {
    turn_ids: HashSet<&'a str>,
    message_ids: HashSet<&'a str>,
}

impl StableMessageKeys<'_> {
    fn is_empty(&self) -> bool {
        self.turn_ids.is_empty() && self.message_ids.is_empty()
    }

    fn contains_event(&self, event: &TalosTimelineEvent) -> bool {
        self.turn_ids.contains(event.turn_id.as_str())
            || self.message_ids.contains(event.message_id.as_str())
    }
}

fn stable_message_keys(messages: &[SessionHistoryMessage]) -> StableMessageKeys<'_> {
    StableMessageKeys {
        turn_ids: messages
            .iter()
            .map(|message| message.turn_id.as_str())
            .filter(|turn_id| !turn_id.is_empty())
            .collect(),
        message_ids: messages
            .iter()
            .map(|message| message.message_id.as_str())
            .filter(|message_id| !message_id.is_empty())
            .collect(),
    }
}

fn merge_completed_timeline_message(
    messages: &mut Vec<SessionHistoryMessage>,
    timeline_message: SessionHistoryMessage,
) {
    if timeline_message.items.is_empty() {
        return;
    }
    if let Some(index) = messages
        .iter()
        .position(|message| message.message_id == timeline_message.message_id)
    {
        messages[index] = timeline_message;
        return;
    }

    let timeline_input = first_input_text(&timeline_message).map(str::to_string);
    messages.retain(|message| {
        !(message.message_id.is_empty()
            && message.stop_reason == StopReason::Cancelled
            && timeline_input
                .as_deref()
                .is_some_and(|input| first_input_text(message) == Some(input)))
    });
    messages.push(timeline_message);
    messages.sort_by_key(|message| {
        message
            .items
            .first()
            .map(|item| item.started_at_ms)
            .unwrap_or_default()
    });
}

fn first_input_text(message: &SessionHistoryMessage) -> Option<&str> {
    message.items.iter().find_map(|item| {
        if item.phase != TalosTimelinePhase::Input {
            return None;
        }
        match &item.content {
            TalosTimelineContent::Text { text } => Some(text.as_str()),
            TalosTimelineContent::Empty
            | TalosTimelineContent::ToolCall { .. }
            | TalosTimelineContent::ExecutionSummary { .. }
            | TalosTimelineContent::SystemNotice { .. } => None,
        }
    })
}

fn history_messages_from_completed_events(
    events: Vec<TalosTimelineEvent>,
) -> Vec<SessionHistoryMessage> {
    let mut messages = Vec::new();
    let mut current = Vec::new();
    let mut current_turn_id = String::new();
    let mut current_message_id = String::new();

    for event in events {
        let same_turn = event.turn_id == current_turn_id && event.message_id == current_message_id;
        if !same_turn && !current.is_empty() {
            if let Some(message) = history_message_from_completed_turn_events(
                &current_turn_id,
                &current_message_id,
                std::mem::take(&mut current),
            ) {
                messages.push(message);
            }
        }
        current_turn_id = event.turn_id.clone();
        current_message_id = event.message_id.clone();
        current.push(event);
    }

    if !current.is_empty() {
        if let Some(message) = history_message_from_completed_turn_events(
            &current_turn_id,
            &current_message_id,
            current,
        ) {
            messages.push(message);
        }
    }

    messages
}

fn history_message_from_completed_turn_events(
    turn_id: &str,
    message_id: &str,
    events: Vec<TalosTimelineEvent>,
) -> Option<SessionHistoryMessage> {
    let mut items: Vec<TalosTimelineItem> = Vec::new();
    let mut usage = TalosUsage::default();
    let mut metrics = None;
    let mut stop_reason = StopReason::EndTurn;
    let mut completed = false;

    for event in events {
        match event.payload {
            TalosTimelineEventPayload::ItemStarted(started) => {
                items.push(started.item);
            }
            TalosTimelineEventPayload::ItemDelta(delta) => {
                if let Some(item) = items.iter_mut().find(|item| item.item_id == delta.item_id) {
                    apply_history_delta(item, delta.delta);
                }
            }
            TalosTimelineEventPayload::ItemCompleted(item_completed) => {
                if let Some(item) = items
                    .iter_mut()
                    .find(|item| item.item_id == item_completed.item_id)
                {
                    item.status = item_completed.status;
                    item.completed_at_ms = Some(item_completed.completed_at_ms);
                    if let Some(final_content) = item_completed.final_content {
                        item.content = final_content;
                    }
                }
            }
            TalosTimelineEventPayload::AssistantTextCommitted(committed) => {
                if let Some(item) = items
                    .iter_mut()
                    .find(|item| item.item_id == committed.item_id)
                {
                    item.phase = committed.phase;
                }
            }
            TalosTimelineEventPayload::TurnCompleted(turn_completed) => {
                usage = turn_completed.usage;
                metrics = turn_completed.metrics;
                stop_reason = turn_completed.stop_reason;
                completed = true;
            }
            TalosTimelineEventPayload::TurnStarted(_)
            | TalosTimelineEventPayload::TurnMetrics(_)
            | TalosTimelineEventPayload::TurnError(_) => {}
        }
    }

    if !completed || items.is_empty() {
        return None;
    }

    Some(SessionHistoryMessage {
        message_id: message_id.to_string(),
        turn_id: turn_id.to_string(),
        items,
        stop_reason,
        usage,
        metrics,
    })
}

fn apply_history_delta(item: &mut TalosTimelineItem, delta: TalosTimelineDelta) {
    match delta {
        TalosTimelineDelta::Text { text } => match &mut item.content {
            TalosTimelineContent::Text { text: existing } => existing.push_str(&text),
            TalosTimelineContent::Empty => {
                item.content = TalosTimelineContent::Text { text };
            }
            TalosTimelineContent::ToolCall { .. }
            | TalosTimelineContent::ExecutionSummary { .. }
            | TalosTimelineContent::SystemNotice { .. } => {}
        },
        TalosTimelineDelta::ToolArguments { arguments_delta } => {
            if let TalosTimelineContent::ToolCall { arguments, .. } = &mut item.content {
                arguments.push_str(&arguments_delta);
            }
        }
        TalosTimelineDelta::ToolProgress {
            progress_kind,
            text_delta,
        } => {
            if let TalosTimelineContent::ToolCall { progress, .. } = &mut item.content {
                progress.push(TalosToolProgressEntry {
                    progress_kind,
                    text: text_delta,
                    timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
                });
            }
        }
        TalosTimelineDelta::ToolResult { content, is_error } => {
            if let TalosTimelineContent::ToolCall { result, .. } = &mut item.content {
                *result = Some(TalosToolResult { content, is_error });
            }
        }
        TalosTimelineDelta::ToolDynamicRequest { .. } => {
            item.status = TalosTimelineItemStatus::WaitingDynamicInput;
        }
        TalosTimelineDelta::Status { status } => {
            item.status = status;
        }
    }
}

fn map_turns_to_history_messages(turns: Vec<TalosTurn>) -> Vec<SessionHistoryMessage> {
    turns
        .into_iter()
        .map(|turn| {
            let items = map_turn_items(&turn);
            SessionHistoryMessage {
                message_id: turn.message_id,
                turn_id: turn.turn_id,
                items,
                stop_reason: turn.stop_reason,
                usage: turn.usage,
                metrics: None,
            }
        })
        .collect()
}

fn map_turn_items(turn: &TalosTurn) -> Vec<TalosTimelineItem> {
    let timestamp_ms = turn.timestamp.timestamp_millis().max(0) as u64;
    let mut items: Vec<TalosTimelineItem> = Vec::new();

    for (index, output) in turn.items.iter().enumerate() {
        match output {
            TalosOutputItem::UserInput { text } => {
                items.push(history_text_item(
                    turn,
                    index,
                    TalosTimelineItemKind::UserInput,
                    TalosTimelinePhase::Input,
                    None,
                    text.clone(),
                    timestamp_ms,
                ));
            }
            TalosOutputItem::Reasoning { text } => {
                items.push(history_text_item(
                    turn,
                    index,
                    TalosTimelineItemKind::Reasoning,
                    TalosTimelinePhase::Thinking,
                    Some("Thinking".to_string()),
                    text.clone(),
                    timestamp_ms,
                ));
            }
            TalosOutputItem::ExecutionSummary { text } => {
                items.push(TalosTimelineItem {
                    item_id: history_item_id(turn, index, "summary"),
                    parent_item_id: None,
                    kind: TalosTimelineItemKind::ExecutionSummary,
                    phase: TalosTimelinePhase::System,
                    status: TalosTimelineItemStatus::Completed,
                    title: Some("Execution summary".to_string()),
                    started_at_ms: timestamp_ms,
                    completed_at_ms: Some(timestamp_ms),
                    content: TalosTimelineContent::ExecutionSummary {
                        summary: text.clone(),
                    },
                });
            }
            TalosOutputItem::OutputText { text } => {
                items.push(history_text_item(
                    turn,
                    index,
                    TalosTimelineItemKind::AssistantText,
                    TalosTimelinePhase::FinalAnswer,
                    None,
                    text.clone(),
                    timestamp_ms,
                ));
            }
            TalosOutputItem::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                items.push(TalosTimelineItem {
                    item_id: history_item_id(turn, index, "tool"),
                    parent_item_id: None,
                    kind: TalosTimelineItemKind::ToolCall,
                    phase: TalosTimelinePhase::Acting,
                    status: TalosTimelineItemStatus::Completed,
                    title: None,
                    started_at_ms: timestamp_ms,
                    completed_at_ms: Some(timestamp_ms),
                    content: TalosTimelineContent::ToolCall {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments: arguments.to_string(),
                        progress: Vec::new(),
                        result: None,
                    },
                });
            }
            TalosOutputItem::ToolResult {
                call_id,
                tool_name,
                content,
                is_error,
            } => {
                if let Some(tool_item) = items.iter_mut().rev().find(|item| {
                    matches!(
                        &item.content,
                        TalosTimelineContent::ToolCall { call_id: existing, .. }
                            if existing == call_id
                    )
                }) {
                    tool_item.status = if *is_error {
                        TalosTimelineItemStatus::Failed
                    } else {
                        TalosTimelineItemStatus::Completed
                    };
                    if let TalosTimelineContent::ToolCall { result, .. } = &mut tool_item.content {
                        *result = Some(TalosToolResult {
                            content: content.clone(),
                            is_error: *is_error,
                        });
                    }
                } else {
                    items.push(history_tool_result_item(
                        turn,
                        index,
                        call_id,
                        tool_name,
                        content,
                        *is_error,
                        timestamp_ms,
                    ));
                }
            }
        }
    }

    items
}

fn history_text_item(
    turn: &TalosTurn,
    index: usize,
    kind: TalosTimelineItemKind,
    phase: TalosTimelinePhase,
    title: Option<String>,
    text: String,
    timestamp_ms: u64,
) -> TalosTimelineItem {
    TalosTimelineItem {
        item_id: history_item_id(turn, index, item_prefix(kind)),
        parent_item_id: None,
        kind,
        phase,
        status: TalosTimelineItemStatus::Completed,
        title,
        started_at_ms: timestamp_ms,
        completed_at_ms: Some(timestamp_ms),
        content: TalosTimelineContent::Text { text },
    }
}

fn history_tool_result_item(
    turn: &TalosTurn,
    index: usize,
    call_id: &str,
    tool_name: &str,
    content: &[TalosContentItem],
    is_error: bool,
    timestamp_ms: u64,
) -> TalosTimelineItem {
    TalosTimelineItem {
        item_id: history_item_id(turn, index, "tool"),
        parent_item_id: None,
        kind: TalosTimelineItemKind::ToolCall,
        phase: TalosTimelinePhase::Acting,
        status: if is_error {
            TalosTimelineItemStatus::Failed
        } else {
            TalosTimelineItemStatus::Completed
        },
        title: None,
        started_at_ms: timestamp_ms,
        completed_at_ms: Some(timestamp_ms),
        content: TalosTimelineContent::ToolCall {
            call_id: call_id.to_string(),
            name: tool_name.to_string(),
            arguments: String::new(),
            progress: Vec::new(),
            result: Some(TalosToolResult {
                content: content.to_vec(),
                is_error,
            }),
        },
    }
}

fn history_item_id(turn: &TalosTurn, index: usize, prefix: &str) -> String {
    format!("history-{}-{prefix}-{index}", turn.turn_id)
}

fn item_prefix(kind: TalosTimelineItemKind) -> &'static str {
    match kind {
        TalosTimelineItemKind::UserInput => "input",
        TalosTimelineItemKind::Reasoning => "reasoning",
        TalosTimelineItemKind::AssistantText => "assistant",
        TalosTimelineItemKind::ToolCall => "tool",
        TalosTimelineItemKind::ExecutionSummary => "summary",
        TalosTimelineItemKind::SystemNotice => "system",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use talos_types::talos::{StopReason, TalosJsonValue, TalosUsage};

    #[test]
    fn history_messages_reuse_timeline_items() {
        let turn = TalosTurn {
            turn_id: "turn-1".to_string(),
            message_id: "message-1".to_string(),
            session_id: "session-1".to_string(),
            seq: 1,
            timestamp: Utc::now(),
            items: vec![
                TalosOutputItem::UserInput {
                    text: "question".to_string(),
                },
                TalosOutputItem::Reasoning {
                    text: "thinking".to_string(),
                },
                TalosOutputItem::ToolCall {
                    call_id: "call-1".to_string(),
                    name: "Read".to_string(),
                    arguments: TalosJsonValue::from(serde_json::json!({"path": "a"})),
                },
                TalosOutputItem::ToolResult {
                    call_id: "call-1".to_string(),
                    tool_name: "Read".to_string(),
                    content: vec![TalosContentItem::Text {
                        text: "content".to_string(),
                    }],
                    is_error: false,
                },
                TalosOutputItem::OutputText {
                    text: "answer".to_string(),
                },
            ],
            usage: TalosUsage::default(),
            stop_reason: StopReason::EndTurn,
        };

        let messages = map_turns_to_history_messages(vec![turn]);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].items.len(), 4);
        assert_eq!(messages[0].items[0].phase, TalosTimelinePhase::Input);
        assert_eq!(messages[0].items[1].phase, TalosTimelinePhase::Thinking);
        assert_eq!(messages[0].items[2].phase, TalosTimelinePhase::Acting);
        assert_eq!(messages[0].items[3].phase, TalosTimelinePhase::FinalAnswer);
        match &messages[0].items[2].content {
            TalosTimelineContent::ToolCall { result, .. } => assert!(result.is_some()),
            _ => panic!("expected tool call content"),
        }
    }

    #[test]
    fn timeline_message_replaces_memory_message_for_item_timing() {
        let mut messages = vec![SessionHistoryMessage {
            message_id: "message-1".to_string(),
            turn_id: "turn-1".to_string(),
            items: vec![
                test_text_item(
                    "memory-input",
                    TalosTimelinePhase::Input,
                    TalosTimelineItemKind::UserInput,
                    100,
                    100,
                ),
                test_text_item(
                    "memory-thinking",
                    TalosTimelinePhase::Thinking,
                    TalosTimelineItemKind::Reasoning,
                    100,
                    100,
                ),
            ],
            usage: TalosUsage::default(),
            metrics: None,
            stop_reason: StopReason::EndTurn,
        }];
        let timeline_message = SessionHistoryMessage {
            message_id: "message-1".to_string(),
            turn_id: "turn-1".to_string(),
            items: vec![
                test_text_item(
                    "event-input",
                    TalosTimelinePhase::Input,
                    TalosTimelineItemKind::UserInput,
                    100,
                    100,
                ),
                test_text_item(
                    "event-thinking",
                    TalosTimelinePhase::Thinking,
                    TalosTimelineItemKind::Reasoning,
                    120,
                    240,
                ),
            ],
            usage: TalosUsage::default(),
            metrics: None,
            stop_reason: StopReason::EndTurn,
        };

        merge_completed_timeline_message(&mut messages, timeline_message);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].items[1].item_id, "event-thinking");
        assert_eq!(messages[0].items[1].started_at_ms, 120);
        assert_eq!(messages[0].items[1].completed_at_ms, Some(240));
    }

    #[test]
    fn stable_message_keys_ignore_active_memory_only_turns() {
        let completed_timeline_messages = vec![test_history_message("message-1", "turn-1")];
        let keys = stable_message_keys(&completed_timeline_messages);

        assert!(keys.contains_event(&test_timeline_event("message-1", "turn-1", 1)));
        assert!(!keys.contains_event(&test_timeline_event("message-active", "turn-active", 2)));
    }

    fn test_history_message(message_id: &str, turn_id: &str) -> SessionHistoryMessage {
        SessionHistoryMessage {
            message_id: message_id.to_string(),
            turn_id: turn_id.to_string(),
            items: vec![test_text_item(
                "input-1",
                TalosTimelinePhase::Input,
                TalosTimelineItemKind::UserInput,
                100,
                100,
            )],
            usage: TalosUsage::default(),
            metrics: None,
            stop_reason: StopReason::EndTurn,
        }
    }

    fn test_timeline_event(message_id: &str, turn_id: &str, sequence: u64) -> TalosTimelineEvent {
        TalosTimelineEvent {
            event_seq: Some(sequence as i64),
            event_id: Some(format!("ev_{sequence}")),
            created_at_ms: Some(sequence as i64),
            event_type: talos_types::talos::TalosTimelineEventType::TurnStarted,
            turn_id: turn_id.to_string(),
            message_id: message_id.to_string(),
            session_id: "session-1".to_string(),
            sequence,
            payload: TalosTimelineEventPayload::TurnStarted(
                talos_types::talos::TalosTurnStartedEvent {
                    request_id: "request-1".to_string(),
                    turn_id: turn_id.to_string(),
                    started_at_ms: sequence,
                },
            ),
        }
    }

    fn test_text_item(
        item_id: &str,
        phase: TalosTimelinePhase,
        kind: TalosTimelineItemKind,
        started_at_ms: u64,
        completed_at_ms: u64,
    ) -> TalosTimelineItem {
        TalosTimelineItem {
            item_id: item_id.to_string(),
            parent_item_id: None,
            kind,
            phase,
            status: TalosTimelineItemStatus::Completed,
            title: None,
            started_at_ms,
            completed_at_ms: Some(completed_at_ms),
            content: TalosTimelineContent::Text {
                text: item_id.to_string(),
            },
        }
    }
}
