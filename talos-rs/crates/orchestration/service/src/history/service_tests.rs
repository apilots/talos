use super::*;
use crate::history::timeline_items_from_events;
use chrono::Utc;
use talos_agent_kernel::TimelineBuilder;
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
                file_ids: vec!["file-1".to_string()],
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
    assert_eq!(
        messages[0].items[0].content,
        TalosTimelineContent::UserInput {
            text: "question".to_string(),
            file_ids: vec!["file-1".to_string()],
        }
    );
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
fn timeline_message_replaces_all_artifact_messages_with_the_same_message_id() {
    let unrelated_before = test_history_message("message-before", "turn-before");
    let artifact_request = test_history_message("message-1", "artifact-request");
    let artifact_resume = test_history_message("message-1", "artifact-resume");
    let unrelated_after = test_history_message("message-after", "turn-after");
    let timeline_message = test_history_message("message-1", "timeline-turn");
    let mut messages = vec![
        unrelated_before.clone(),
        artifact_request,
        artifact_resume,
        unrelated_after.clone(),
    ];

    merge_completed_timeline_message(&mut messages, timeline_message.clone());

    assert_eq!(
        messages,
        vec![unrelated_before, timeline_message, unrelated_after]
    );
}

#[test]
fn timeline_replay_applies_reused_item_ids_to_the_latest_lifecycle() {
    let mut initial = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let initial_started = initial.start_reasoning();
    initial
        .append_delta(
            "reasoning-1",
            talos_types::talos::TalosTimelineDelta::Text {
                text: "initial".to_string(),
            },
        )
        .expect("initial delta should apply");
    let initial_completed = initial
        .complete_item("reasoning-1", TalosTimelineItemStatus::Completed, None)
        .expect("initial item should complete");

    let mut resumed = TimelineBuilder::new("request-2", "turn-1", "message-1", "session-1");
    let resumed_started = resumed.start_reasoning();
    let resumed_delta = resumed
        .append_delta(
            "reasoning-1",
            talos_types::talos::TalosTimelineDelta::Text {
                text: "resumed".to_string(),
            },
        )
        .expect("resumed delta should apply");
    let resumed_completed = resumed
        .complete_item("reasoning-1", TalosTimelineItemStatus::Completed, None)
        .expect("resumed item should complete");

    let items = timeline_items_from_events(&[
        initial_started,
        initial_completed,
        resumed_started,
        resumed_delta,
        resumed_completed,
    ]);

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].status, TalosTimelineItemStatus::Completed);
    assert_eq!(items[1].status, TalosTimelineItemStatus::Completed);
    assert_eq!(
        items[1].content,
        TalosTimelineContent::Text {
            text: "resumed".to_string(),
        }
    );
}

#[test]
fn stable_message_keys_ignore_active_memory_only_turns() {
    let completed_timeline_messages = vec![test_history_message("message-1", "turn-1")];
    let keys = stable_message_keys(&completed_timeline_messages);

    assert!(keys.contains_event(&test_timeline_event("message-1", "turn-1", 1)));
    assert!(!keys.contains_event(&test_timeline_event("message-active", "turn-active", 2)));
}

#[test]
fn waiting_turn_is_not_stable_until_resumed_tools_reach_terminal_state() {
    let mut waiting = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let mut events = vec![waiting.start_turn()];
    let input = waiting.start_user_input("question", Vec::new());
    let input_id = timeline_item_id(&input);
    events.push(input);
    events.push(
        waiting
            .complete_item(&input_id, TalosTimelineItemStatus::Completed, None)
            .expect("input should complete"),
    );
    let tool = waiting.start_tool_call("call-1", "Bash", "{}");
    let tool_id = timeline_item_id(&tool);
    events.push(tool);
    events.push(
        waiting
            .append_delta(
                &tool_id,
                talos_types::talos::TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::WaitingUserAction,
                },
            )
            .expect("tool should wait"),
    );
    events.push(waiting.complete_turn(
        StopReason::WaitingUserAction,
        TokenUsage::default(),
        None,
        None,
    ));

    assert!(history_messages_from_completed_events(events.clone()).is_empty());

    let mut resumed = TimelineBuilder::new("request-2", "turn-1", "message-1", "session-1");
    resumed.track_existing_tool_call(&tool_id, "call-1", "Bash", "{}");
    events.push(resumed.start_turn());
    events.push(
        resumed
            .append_delta(
                &tool_id,
                talos_types::talos::TalosTimelineDelta::ToolResult {
                    content: vec![TalosContentItem::Text {
                        text: "done".to_string(),
                    }],
                    is_error: false,
                },
            )
            .expect("tool result should apply"),
    );
    events.push(
        resumed
            .complete_item(&tool_id, TalosTimelineItemStatus::Completed, None)
            .expect("tool should complete"),
    );
    let answer = resumed.start_item(
        TalosTimelineItemKind::AssistantText,
        TalosTimelinePhase::FinalAnswer,
        None,
        TalosTimelineContent::Text {
            text: "answer".to_string(),
        },
    );
    let answer_id = timeline_item_id(&answer);
    events.push(answer);
    events.push(
        resumed
            .complete_item(&answer_id, TalosTimelineItemStatus::Completed, None)
            .expect("answer should complete"),
    );
    events.push(resumed.complete_turn(StopReason::EndTurn, TokenUsage::default(), None, None));

    let messages = history_messages_from_completed_events(events);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].stop_reason, StopReason::EndTurn);
    assert!(messages[0].items.iter().all(|item| matches!(
        item.status,
        TalosTimelineItemStatus::Completed
            | TalosTimelineItemStatus::Failed
            | TalosTimelineItemStatus::Cancelled
    )));
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

fn timeline_item_id(event: &AgentEvent) -> String {
    match &event.payload {
        AgentEventPayload::ItemStarted(started) => started.item.item_id.clone(),
        _ => panic!("expected item started event"),
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
