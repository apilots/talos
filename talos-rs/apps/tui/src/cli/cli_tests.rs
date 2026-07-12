use super::*;
use std::collections::{HashMap, HashSet};
use talos_sdk::{
    AgentEvent, AgentEventPayload, AgentEventType, StopReason, TalosItemCompletedEvent,
    TalosTimelineContent, TalosTimelineItemStatus, TalosTimelinePhase, TalosTurnCompletedEvent,
    TalosTurnErrorEvent, TimelineEventEnvelope, TokenUsage,
};

#[test]
fn parses_exec_command() {
    let cli = Cli::try_parse_from(["talos", "exec", "hello"]);
    assert!(cli.is_ok());
}

#[test]
fn parses_tui_default_command() {
    let cli = Cli::try_parse_from(["talos"]);
    assert!(cli.is_ok());
}

#[test]
fn render_event_keeps_exec_open_on_final_answer_completion() {
    let mut item_phases = HashMap::from([("final-1".to_string(), TalosTimelinePhase::FinalAnswer)]);
    let mut item_texts = HashMap::new();
    let mut written_final_items = HashSet::new();
    let mut output_text = String::new();
    let mut stdout = std::io::stdout();
    let envelope = TimelineEventEnvelope {
        event_id: "event-1".to_string(),
        event_seq: 1,
        created_at_ms: 1,
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        event: AgentEvent {
            event_seq: Some(1),
            event_id: Some("event-1".to_string()),
            created_at_ms: Some(1),
            event_type: AgentEventType::ItemCompleted,
            turn_id: "turn-1".to_string(),
            message_id: "message-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 1,
            payload: AgentEventPayload::ItemCompleted(TalosItemCompletedEvent {
                request_id: "request-1".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "final-1".to_string(),
                status: TalosTimelineItemStatus::Completed,
                completed_at_ms: 1,
                final_content: Some(TalosTimelineContent::Text {
                    text: "done".to_string(),
                }),
            }),
        },
    };

    let should_finish = render_event(
        &envelope,
        &mut item_phases,
        &mut item_texts,
        &mut written_final_items,
        &mut output_text,
        &mut stdout,
    )
    .expect("final answer completion should render");

    assert!(!should_finish);
    assert_eq!(output_text, "done");
}

#[test]
fn render_event_finishes_exec_on_turn_completed() {
    let mut item_phases = HashMap::new();
    let mut item_texts = HashMap::new();
    let mut written_final_items = HashSet::new();
    let mut output_text = String::new();
    let mut stdout = std::io::stdout();
    let envelope = TimelineEventEnvelope {
        event_id: "event-1".to_string(),
        event_seq: 1,
        created_at_ms: 1,
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        event: AgentEvent {
            event_seq: Some(1),
            event_id: Some("event-1".to_string()),
            created_at_ms: Some(1),
            event_type: AgentEventType::TurnCompleted,
            turn_id: "turn-1".to_string(),
            message_id: "message-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 1,
            payload: AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "request-1".to_string(),
                turn_id: "turn-1".to_string(),
                stop_reason: StopReason::EndTurn,
                usage: TokenUsage::default(),
                metrics: None,
                completed_at_ms: 1,
            }),
        },
    };

    let should_finish = render_event(
        &envelope,
        &mut item_phases,
        &mut item_texts,
        &mut written_final_items,
        &mut output_text,
        &mut stdout,
    )
    .expect("turn completion should finish");

    assert!(should_finish);
}

#[test]
fn render_event_returns_turn_error() {
    let mut item_phases = HashMap::new();
    let mut item_texts = HashMap::new();
    let mut written_final_items = HashSet::new();
    let mut output_text = String::new();
    let mut stdout = std::io::stdout();
    let envelope = TimelineEventEnvelope {
        event_id: "event-1".to_string(),
        event_seq: 1,
        created_at_ms: 1,
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        event: AgentEvent {
            event_seq: Some(1),
            event_id: Some("event-1".to_string()),
            created_at_ms: Some(1),
            event_type: AgentEventType::TurnError,
            turn_id: "turn-1".to_string(),
            message_id: "message-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 1,
            payload: AgentEventPayload::TurnError(TalosTurnErrorEvent {
                request_id: "request-1".to_string(),
                turn_id: "turn-1".to_string(),
                error: "provider request failed".to_string(),
                error_info: None,
                failed_at_ms: 1,
            }),
        },
    };

    let error = render_event(
        &envelope,
        &mut item_phases,
        &mut item_texts,
        &mut written_final_items,
        &mut output_text,
        &mut stdout,
    )
    .expect_err("turn error should fail exec mode");

    assert_eq!(error.to_string(), "provider request failed");
}
