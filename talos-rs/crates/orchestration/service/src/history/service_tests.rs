use super::*;
use chrono::Utc;
use talos_store::session::TalosTurn;
use talos_types::talos::{
    AgentEvent, AgentEventPayload, RunItem, SessionHistoryMessage, StopReason, TalosContentItem,
    TalosTimelineContent, TalosTimelineItem, TalosTimelineItemKind, TalosTimelineItemStatus,
    TalosTimelinePhase, TokenUsage, ToolArguments,
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
            RunItem::UserInput {
                text: "question".to_string(),
            },
            RunItem::Reasoning {
                text: "thinking".to_string(),
            },
            RunItem::ToolCall {
                call_id: "call-1".to_string(),
                name: "Read".to_string(),
                arguments: ToolArguments::from(serde_json::json!({"path": "a"})),
            },
            RunItem::ToolResult {
                call_id: "call-1".to_string(),
                tool_name: "Read".to_string(),
                content: vec![TalosContentItem::Text {
                    text: "content".to_string(),
                }],
                is_error: false,
            },
            RunItem::OutputText {
                text: "answer".to_string(),
            },
        ],
        usage: TokenUsage::default(),
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
        usage: TokenUsage::default(),
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
        usage: TokenUsage::default(),
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
        usage: TokenUsage::default(),
        metrics: None,
        stop_reason: StopReason::EndTurn,
    }
}

fn test_timeline_event(message_id: &str, turn_id: &str, sequence: u64) -> AgentEvent {
    AgentEvent {
        event_seq: Some(sequence as i64),
        event_id: Some(format!("ev_{sequence}")),
        created_at_ms: Some(sequence as i64),
        event_type: talos_types::talos::AgentEventType::TurnStarted,
        turn_id: turn_id.to_string(),
        message_id: message_id.to_string(),
        session_id: "session-1".to_string(),
        sequence,
        payload: AgentEventPayload::TurnStarted(talos_types::talos::TalosTurnStartedEvent {
            request_id: "request-1".to_string(),
            turn_id: turn_id.to_string(),
            started_at_ms: sequence,
        }),
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
