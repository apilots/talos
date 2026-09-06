use super::*;
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentMessageBlockCompleted,
    AgentMessageBlockContent, AgentMessageBlockStarted, AgentMessageRole, AgentMessageStarted,
    AgentPendingInput, AgentStopReason, AgentToolCall, AgentToolCallArguments,
    AgentToolCallArgumentsDelta, AgentTurnCompleted, AgentTurnStarted,
};
use talos_types::{DynamicToolResultParams, PendingUserAction, UserActionParams, UserActionState};

fn event(sequence: u64, payload: AgentEventPayload) -> AgentEvent {
    AgentEvent {
        identity: AgentTurnRef::new("session-1", "turn-1"),
        sequence,
        occurred_at: 100 + i64::try_from(sequence).unwrap(),
        payload,
    }
}

fn started_message() -> AgentMessage {
    AgentMessage {
        message_id: "message-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::Agent,
        status: AgentMessageStatus::InProgress,
        blocks: Vec::new(),
        created_at: 101,
        completed_at: None,
    }
}

fn started_block() -> AgentMessageBlock {
    AgentMessageBlock {
        block_id: "block-1".to_string(),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: String::new(),
        }),
        started_at: 102,
        completed_at: None,
    }
}

#[test]
fn reducer_rejects_sequence_gaps() {
    let mut reducer = AgentEventReducer::new(AgentTurnRef::new("session-1", "turn-1"));

    let error = reducer
        .apply(event(
            2,
            AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        ))
        .expect_err("sequence gap must fail");

    assert!(error.message.contains("expected 1"));
}

#[test]
fn reducer_rejects_delta_for_the_wrong_block_kind() {
    let mut reducer = AgentEventReducer::new(AgentTurnRef::new("session-1", "turn-1"));
    reducer
        .apply(event(
            1,
            AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        ))
        .unwrap();
    reducer
        .apply(event(
            2,
            AgentEventPayload::MessageStarted(AgentMessageStarted {
                message: started_message(),
            }),
        ))
        .unwrap();
    reducer
        .apply(event(
            3,
            AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: "message-1".to_string(),
                block: started_block(),
            }),
        ))
        .unwrap();

    let error = reducer
        .apply(event(
            4,
            AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
                message_id: "message-1".to_string(),
                block_id: "block-1".to_string(),
                delta: AgentMessageBlockDeltaContent::ToolCallArguments(
                    AgentToolCallArgumentsDelta {
                        json_fragment: "{}".to_string(),
                    },
                ),
            }),
        ))
        .expect_err("Tool argument delta must not target assistant text");

    assert!(error.message.contains("does not match"));
}

#[test]
fn reducer_builds_one_canonical_message_checkpoint() {
    let mut reducer = AgentEventReducer::new(AgentTurnRef::new("session-1", "turn-1"));
    reducer
        .apply(event(
            1,
            AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        ))
        .unwrap();
    reducer
        .apply(event(
            2,
            AgentEventPayload::MessageStarted(AgentMessageStarted {
                message: started_message(),
            }),
        ))
        .unwrap();
    reducer
        .apply(event(
            3,
            AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: "message-1".to_string(),
                block: started_block(),
            }),
        ))
        .unwrap();
    reducer
        .apply(event(
            4,
            AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
                message_id: "message-1".to_string(),
                block_id: "block-1".to_string(),
                delta: AgentMessageBlockDeltaContent::Text("done".to_string()),
            }),
        ))
        .unwrap();

    let mut completed_block = started_block();
    completed_block.status = AgentMessageBlockStatus::Completed;
    completed_block.completed_at = Some(105);
    if let AgentMessageBlockContent::AssistantText(content) = &mut completed_block.content {
        content.text = "done".to_string();
    }
    reducer
        .apply(event(
            5,
            AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
                message_id: "message-1".to_string(),
                block: completed_block.clone(),
            }),
        ))
        .unwrap();

    let mut completed_message = started_message();
    completed_message.status = AgentMessageStatus::Completed;
    completed_message.blocks.push(completed_block);
    completed_message.completed_at = Some(106);
    reducer
        .apply(event(
            6,
            AgentEventPayload::MessageCompleted(completed_message.clone()),
        ))
        .unwrap();
    reducer
        .apply(event(
            7,
            AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        ))
        .unwrap();

    assert_eq!(
        reducer.checkpoint(),
        AgentRunCheckpoint {
            last_sequence: 7,
            messages: vec![completed_message],
            metrics: AgentRunMetrics::default(),
        }
    );
}

#[test]
fn waiting_user_action_marks_the_referenced_block_without_a_second_status_event() {
    let mut reducer = AgentEventReducer::new(AgentTurnRef::new("session-1", "turn-1"));
    reducer
        .apply(event(
            1,
            AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        ))
        .unwrap();
    reducer
        .apply(event(
            2,
            AgentEventPayload::MessageStarted(AgentMessageStarted {
                message: started_message(),
            }),
        ))
        .unwrap();
    reducer
        .apply(event(
            3,
            AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: "message-1".to_string(),
                block: started_block(),
            }),
        ))
        .unwrap();

    reducer
        .apply(event(
            4,
            AgentEventPayload::WaitingForInput(AgentPendingInput::UserAction {
                action: PendingUserAction {
                    action_id: "action-1".to_string(),
                    call_id: "call-1".to_string(),
                    item_id: Some("block-1".to_string()),
                    state: UserActionState::Pending,
                    params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
                        tool_name: "search".to_string(),
                    }),
                },
            }),
        ))
        .unwrap();

    assert_eq!(
        reducer.checkpoint().messages[0].blocks[0].status,
        AgentMessageBlockStatus::WaitingForInput
    );
}

#[test]
fn failed_tool_block_preserves_incomplete_streaming_arguments() {
    let mut reducer = AgentEventReducer::new(AgentTurnRef::new("session-1", "turn-1"));
    reducer
        .apply(event(
            1,
            AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        ))
        .unwrap();
    reducer
        .apply(event(
            2,
            AgentEventPayload::MessageStarted(AgentMessageStarted {
                message: started_message(),
            }),
        ))
        .unwrap();
    let started = AgentMessageBlock {
        block_id: "tool-block-1".to_string(),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::ToolCall(AgentToolCall {
            call_id: "call-1".to_string(),
            name: "search".to_string(),
            arguments: AgentToolCallArguments::Streaming {
                json_fragment: "{\"query\":".to_string(),
            },
            progress: Vec::new(),
        }),
        started_at: 103,
        completed_at: None,
    };
    reducer
        .apply(event(
            3,
            AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: "message-1".to_string(),
                block: started.clone(),
            }),
        ))
        .unwrap();

    let failed = reducer
        .completed_block_snapshot(
            "message-1",
            "tool-block-1",
            AgentMessageBlockStatus::Failed,
            104,
        )
        .unwrap();

    assert_eq!(
        failed,
        AgentMessageBlock {
            status: AgentMessageBlockStatus::Failed,
            completed_at: Some(104),
            ..started
        }
    );
}
