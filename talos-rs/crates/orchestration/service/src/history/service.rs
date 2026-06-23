use std::sync::Arc;

use talos_error::TalosResult;
use talos_memory::MemoryStore;
use talos_store::session::TimelineEventId;
use talos_types::talos::{SessionHistoryMessage, SessionHistorySnapshot};
use talos_types::TalosConfig;

use super::message_merge::{
    history_messages_from_completed_events, map_turns_to_history_messages,
    merge_completed_timeline_message,
};
use super::stable_keys::stable_message_keys;
use crate::TimelineEventService;

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use talos_memory::TalosTurn;
    use talos_types::talos::{
        SessionHistoryMessage, StopReason, TalosContentItem, TalosOutputItem, TalosTimelineContent,
        TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineItem, TalosTimelineItemKind,
        TalosTimelineItemStatus, TalosTimelinePhase, TalosUsage, ToolArguments,
    };

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
                    arguments: ToolArguments::from(serde_json::json!({"path": "a"})),
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
