use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockStatus, AgentMessageRole, AgentMessageStatus, AgentPendingInput,
    AgentRunMetrics, AgentToolCall, AgentToolCallArguments, AgentToolResult, AgentTurnFailed,
};
use talos_error::{ErrorCode, TalosError};
use talos_types::{
    AgentPath, AgentTurnRef, AgentUserContent, CollaborationToolResult, JsonContentValue,
    PendingUserAction, SandboxApprovalDecision, SandboxApprovalParams, SandboxApprovalPresentation,
    SandboxApprovalReason, SubagentOutcome, SubagentRef, SubagentSnapshot, TalosContentItem,
    ToolArguments, UserActionParams, UserActionState,
};

use super::*;
use crate::AgentRunSettlement;

#[test]
fn bash_log_and_file_byte_cursor_survive_protobuf() {
    use talos_types::talos::{BashOutputCaptureStatus as S, BashOutputLog};
    for capture_status in [
        S::Capturing,
        S::Complete,
        S::Interrupted,
        S::QuotaLimited,
        S::WriteFailed,
    ] {
        let item = TalosContentItem::BashOutput {
            session_id: None,
            status: talos_types::TalosToolTaskStatus::Failed,
            exit_code: Some(7),
            stdout: "tail".into(),
            stderr: String::new(),
            truncated: true,
            elapsed_ms: Some(10),
            output_log: Some(BashOutputLog {
                path: "/workspace/user/tool-output/log".into(),
                relative_path: "tool-output/log".into(),
                total_bytes: 5000000,
                stored_bytes: 1024,
                total_lines: 20,
                capture_status,
                expires_at: 1900000000,
            }),
        };
        assert_eq!(
            decode_tool_content(encode_tool_content(item.clone()).unwrap()).unwrap(),
            item
        );
    }
    let item = TalosContentItem::FileRead {
        path: "output.log".into(),
        offset: 0,
        limit: 0,
        total_lines: None,
        next_byte_offset: Some(4096),
        content: "fragment".into(),
        truncated: true,
    };
    assert_eq!(
        decode_tool_content(encode_tool_content(item.clone()).unwrap()).unwrap(),
        item
    );
}

#[test]
fn rich_message_and_event_survive_typed_protobuf_projection() {
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let child = SubagentRef {
        path: AgentPath::root().join("child").unwrap(),
        session_id: "child-session".into(),
        parent: identity.clone(),
    };
    let message = AgentMessage {
        message_id: "message-1".into(),
        turn_id: "turn-1".into(),
        role: AgentMessageRole::Agent,
        status: AgentMessageStatus::Completed,
        blocks: vec![
            block(
                "user",
                AgentMessageBlockContent::UserContent(AgentUserContent::File {
                    file_id: "file-1".into(),
                }),
            ),
            block(
                "call",
                AgentMessageBlockContent::ToolCall(AgentToolCall {
                    call_id: "call-1".into(),
                    name: "inspect".into(),
                    arguments: AgentToolCallArguments::Complete {
                        value: ToolArguments::new(
                            serde_json::json!({"exact": 9007199254740993_u64}),
                        ),
                    },
                    progress: Vec::new(),
                }),
            ),
            block(
                "result",
                AgentMessageBlockContent::ToolResult(AgentToolResult {
                    metrics: Some(talos_types::ToolExecutionMetrics {
                        queued_at_ms: Some(10000),
                        started_at_ms: Some(10001),
                        finished_at_ms: Some(10003),
                        queue_duration_ms: Some(1.0),
                        execution_duration_ms: Some(2.0),
                    }),
                    call_id: "call-1".into(),
                    tool_name: "inspect".into(),
                    content: vec![
                        TalosContentItem::Json {
                            value: JsonContentValue::new(
                                serde_json::json!({"exact": 9007199254740993_u64}),
                            ),
                        },
                        TalosContentItem::SubagentResult {
                            result: CollaborationToolResult::WaitAgent(SubagentSnapshot {
                                subagent: child,
                                agent_id: "talos".into(),
                                latest_update: None,
                                outcome: SubagentOutcome::Completed {
                                    turn_id: "child-turn".into(),
                                    output: "done".into(),
                                    truncated: false,
                                },
                            }),
                        },
                    ],
                    is_error: false,
                }),
            ),
        ],
        created_at: 1,
        completed_at: Some(2),
    };
    let event = AgentEvent {
        identity,
        sequence: 5,
        occurred_at: 2,
        payload: AgentEventPayload::MessageCompleted(message.clone()),
    };
    assert_eq!(
        decode_message(encode_message(message.clone()).unwrap()).unwrap(),
        message
    );
    assert_eq!(
        decode_event(encode_event(event.clone()).unwrap()).unwrap(),
        event
    );
}

#[test]
fn waiting_and_failed_settlements_keep_business_result_and_user_action() {
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let waiting_event = AgentEvent {
        identity: identity.clone(),
        sequence: 6,
        occurred_at: 3,
        payload: AgentEventPayload::WaitingForInput(AgentPendingInput::UserAction {
            action: PendingUserAction {
                action_id: "action-1".into(),
                call_id: "call-1".into(),
                item_id: None,
                state: UserActionState::Pending,
                params: UserActionParams::SandboxApproval(SandboxApprovalParams {
                    tool_name: "exec".into(),
                    reason: SandboxApprovalReason::NetworkAccess,
                    presentation: SandboxApprovalPresentation::Network {
                        scope: "example.com".into(),
                    },
                    available_decisions: vec![SandboxApprovalDecision::AllowOnce],
                }),
            },
        }),
    };
    let waiting = AgentRunSettlement::WaitingForInput {
        event: waiting_event.clone(),
        continuation: talos_agent_api::AgentContinuationRef {
            opaque_id: "provider-state".into(),
        },
        metrics: AgentRunMetrics::default(),
    };
    match decode_settlement(encode_settlement(waiting).unwrap()).unwrap() {
        AgentRunSettlement::WaitingForInput {
            event,
            continuation,
            ..
        } => {
            assert_eq!(event, waiting_event);
            assert_eq!(continuation.opaque_id, "provider-state");
        }
        _ => panic!("waiting settlement changed variant"),
    }

    let failure = TalosError::new(ErrorCode::ToolExecutionFailed, "Tool failed");
    let failed_event = AgentEvent {
        identity,
        sequence: 7,
        occurred_at: 4,
        payload: AgentEventPayload::TurnFailed(AgentTurnFailed {
            error: failure.to_api_error(),
            retryable: true,
        }),
    };
    let finished = AgentRunSettlement::Finished {
        event: failed_event.clone(),
        messages: Vec::new(),
        metrics: AgentRunMetrics::default(),
        result: Err(failure),
    };
    match decode_settlement(encode_settlement(finished).unwrap()).unwrap() {
        AgentRunSettlement::Finished { event, result, .. } => {
            assert_eq!(event, failed_event);
            assert!(result.unwrap_err().is_code(ErrorCode::ToolExecutionFailed));
        }
        _ => panic!("finished settlement changed variant"),
    }
}

fn block(id: &str, content: AgentMessageBlockContent) -> AgentMessageBlock {
    AgentMessageBlock {
        block_id: id.into(),
        status: AgentMessageBlockStatus::Completed,
        content,
        started_at: 1,
        completed_at: Some(2),
    }
}
