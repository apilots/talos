//! Typed projections for the control-plane data read by a remote Harness.

use agent_pb as pb;
use std::str::FromStr;
use talos_agent_api::{
    AgentRunFailure, AgentRunOutput, AgentRunResult, AgentStopReason, AgentTurnCompleted,
    AgentTurnFailed, SessionHistoryTurn, TimelineEvent, TimelineEventPayload, TimelineSystemNotice,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    AgentCaller, AgentPath, AgentTarget, AgentTreeSnapshot, CollaborationToolResult, FileInfo,
    FileScope, FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest,
    SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest, SubagentSnapshot,
    SubagentSpawnResult, WaitSubagentRequest,
};

use crate::{error_pb, harness_pb};

pub fn decode_spawn_params(
    value: Option<pb::SpawnSubagentParams>,
) -> TalosResult<SpawnSubagentRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(SpawnSubagentRequest {
        caller: decode_caller(value.caller)?,
        task_name: value.task_name,
        prompt: value.prompt,
    })
}

pub fn decode_send_params(
    value: Option<pb::SendSubagentMessageParams>,
) -> TalosResult<SendSubagentMessageRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(SendSubagentMessageRequest {
        caller: decode_caller(value.caller)?,
        target: decode_target(value.target)?,
        message: value.message,
    })
}

pub fn decode_wait_params(
    value: Option<pb::WaitSubagentParams>,
) -> TalosResult<WaitSubagentRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(WaitSubagentRequest {
        caller: decode_caller(value.caller)?,
        target: decode_target(value.target)?,
        timeout_ms: value.timeout_ms,
    })
}

pub fn decode_interrupt_params(
    value: Option<pb::InterruptSubagentParams>,
) -> TalosResult<InterruptSubagentRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(InterruptSubagentRequest {
        caller: decode_caller(value.caller)?,
        target: decode_target(value.target)?,
    })
}

pub fn decode_follow_up_params(
    value: Option<pb::FollowUpSubagentParams>,
) -> TalosResult<FollowUpSubagentRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(FollowUpSubagentRequest {
        caller: decode_caller(value.caller)?,
        target: decode_target(value.target)?,
        prompt: value.prompt,
    })
}

pub fn decode_list_params(
    value: Option<pb::ListSubagentsParams>,
) -> TalosResult<ListSubagentsRequest> {
    Ok(ListSubagentsRequest {
        caller: decode_caller(value.ok_or_else(invalid)?.caller)?,
    })
}

pub fn encode_spawn_result(value: SubagentSpawnResult) -> pb::SubagentSpawnResult {
    match harness_pb::encode_collaboration(CollaborationToolResult::SpawnAgent(value))
        .result
        .expect("collaboration result is present")
    {
        pb::collaboration_result::Result::SpawnAgent(value) => value,
        _ => unreachable!(),
    }
}

pub fn encode_send_result(value: SendMessageResult) -> pb::SendMessageResult {
    match harness_pb::encode_collaboration(CollaborationToolResult::SendMessage(value))
        .result
        .expect("collaboration result is present")
    {
        pb::collaboration_result::Result::SendMessage(value) => value,
        _ => unreachable!(),
    }
}

pub fn encode_snapshot(value: SubagentSnapshot) -> pb::SubagentSnapshot {
    match harness_pb::encode_collaboration(CollaborationToolResult::WaitAgent(value))
        .result
        .expect("collaboration result is present")
    {
        pb::collaboration_result::Result::WaitAgent(value) => value,
        _ => unreachable!(),
    }
}

pub fn encode_tree(value: AgentTreeSnapshot) -> pb::AgentTree {
    match harness_pb::encode_collaboration(CollaborationToolResult::ListAgents(value))
        .result
        .expect("collaboration result is present")
    {
        pb::collaboration_result::Result::ListAgents(value) => value,
        _ => unreachable!(),
    }
}

pub fn encode_tool_file(value: FileInfo) -> TalosResult<pb::ToolFileInfo> {
    if value.scope != FileScope::Personal {
        return Err(invalid());
    }
    Ok(pb::ToolFileInfo {
        file_id: value.file_id,
        original_filename: value.original_filename,
        mime_type: value.mime_type,
        size_bytes: value.size_bytes,
        tags: value.tags,
        metadata: value.metadata.into_iter().collect(),
        created_at: value.created_at,
        updated_at: value.updated_at,
    })
}

pub fn decode_tool_file(value: pb::ToolFileInfo) -> TalosResult<FileInfo> {
    if value.file_id.is_empty() || value.original_filename.is_empty() || value.mime_type.is_empty()
    {
        return Err(invalid());
    }
    Ok(FileInfo {
        file_id: value.file_id,
        scope: FileScope::Personal,
        original_filename: value.original_filename,
        mime_type: value.mime_type,
        size_bytes: value.size_bytes,
        tags: value.tags,
        metadata: value.metadata.into_iter().collect(),
        created_at: value.created_at,
        updated_at: value.updated_at,
    })
}

fn decode_caller(value: Option<pb::AgentCaller>) -> TalosResult<AgentCaller> {
    let value = value.ok_or_else(invalid)?;
    if value.session_id.is_empty() || value.turn_id.is_empty() || value.agent_id.is_empty() {
        return Err(invalid());
    }
    Ok(AgentCaller {
        session_id: value.session_id,
        turn_id: value.turn_id,
        agent_id: value.agent_id,
    })
}

fn decode_target(value: Option<pb::AgentTarget>) -> TalosResult<AgentTarget> {
    match value.and_then(|value| value.target).ok_or_else(invalid)? {
        pb::agent_target::Target::DirectChildName(name) if !name.is_empty() => {
            Ok(AgentTarget::DirectChild { name })
        }
        pb::agent_target::Target::DescendantPath(path) => Ok(AgentTarget::Descendant {
            path: AgentPath::from_str(&path)?,
        }),
        _ => Err(invalid()),
    }
}

pub fn encode_history_turn(value: SessionHistoryTurn) -> TalosResult<pb::SessionHistoryTurn> {
    Ok(pb::SessionHistoryTurn {
        identity: Some(harness_pb::encode_turn_ref(value.identity)),
        result: Some(encode_run_result(value.result)?),
        notices: value
            .notices
            .into_iter()
            .map(encode_timeline_event)
            .collect::<TalosResult<_>>()?,
        completed_event_seq: value.completed_event_seq,
        completed_at: value.completed_at,
    })
}

pub fn decode_history_turn(value: pb::SessionHistoryTurn) -> TalosResult<SessionHistoryTurn> {
    Ok(SessionHistoryTurn {
        identity: harness_pb::decode_turn_ref(value.identity)?,
        result: decode_run_result(value.result.ok_or_else(invalid)?)?,
        notices: value
            .notices
            .into_iter()
            .map(decode_timeline_event)
            .collect::<TalosResult<_>>()?,
        completed_event_seq: value.completed_event_seq,
        completed_at: value.completed_at,
    })
}

fn encode_run_result(value: AgentRunResult) -> TalosResult<pb::HistoryRunResult> {
    let result = match value {
        AgentRunResult::Completed(output) => {
            pb::history_run_result::Result::Completed(pb::HistoryRunOutput {
                messages: output
                    .messages
                    .into_iter()
                    .map(harness_pb::encode_message)
                    .collect::<TalosResult<_>>()?,
                metrics: Some(harness_pb::encode_metrics(output.metrics)),
                stop_reason: encode_stop_reason(output.completion.stop_reason),
            })
        }
        AgentRunResult::Failed(failure) => {
            pb::history_run_result::Result::Failed(pb::HistoryRunFailure {
                messages: failure
                    .messages
                    .into_iter()
                    .map(harness_pb::encode_message)
                    .collect::<TalosResult<_>>()?,
                metrics: Some(harness_pb::encode_metrics(failure.metrics)),
                failure: Some(pb::TurnFailed {
                    error: Some(error_pb::encode_api_error(failure.failure.error)),
                    retryable: failure.failure.retryable,
                }),
            })
        }
    };
    Ok(pb::HistoryRunResult {
        result: Some(result),
    })
}

fn decode_run_result(value: pb::HistoryRunResult) -> TalosResult<AgentRunResult> {
    match value.result.ok_or_else(invalid)? {
        pb::history_run_result::Result::Completed(output) => {
            Ok(AgentRunResult::Completed(AgentRunOutput {
                messages: output
                    .messages
                    .into_iter()
                    .map(harness_pb::decode_message)
                    .collect::<TalosResult<_>>()?,
                metrics: harness_pb::decode_metrics(output.metrics)?,
                completion: AgentTurnCompleted {
                    stop_reason: decode_stop_reason(output.stop_reason)?,
                },
            }))
        }
        pb::history_run_result::Result::Failed(failure) => {
            let terminal = failure.failure.ok_or_else(invalid)?;
            Ok(AgentRunResult::Failed(AgentRunFailure {
                messages: failure
                    .messages
                    .into_iter()
                    .map(harness_pb::decode_message)
                    .collect::<TalosResult<_>>()?,
                metrics: harness_pb::decode_metrics(failure.metrics)?,
                failure: AgentTurnFailed {
                    error: error_pb::decode_api_error(terminal.error.ok_or_else(invalid)?)?,
                    retryable: terminal.retryable,
                },
            }))
        }
    }
}

fn encode_timeline_event(value: TimelineEvent) -> TalosResult<pb::TimelineEvent> {
    let event = match value.event {
        TimelineEventPayload::Agent(event) => {
            pb::timeline_event::Event::Agent(harness_pb::encode_event(event)?)
        }
        TimelineEventPayload::SystemNotice(notice) => {
            pb::timeline_event::Event::SystemNotice(pb::ProductSystemNotice {
                identity: Some(harness_pb::encode_turn_ref(notice.identity)),
                occurred_at: notice.occurred_at,
                notice: Some(harness_pb::encode_system_notice(notice.notice)),
            })
        }
    };
    Ok(pb::TimelineEvent {
        event_id: value.event_id,
        event_seq: value.event_seq,
        persisted_at: value.persisted_at,
        event: Some(event),
    })
}

fn decode_timeline_event(value: pb::TimelineEvent) -> TalosResult<TimelineEvent> {
    let event = match value.event.ok_or_else(invalid)? {
        pb::timeline_event::Event::Agent(event) => {
            TimelineEventPayload::Agent(harness_pb::decode_event(event)?)
        }
        pb::timeline_event::Event::SystemNotice(notice) => {
            TimelineEventPayload::SystemNotice(TimelineSystemNotice {
                identity: harness_pb::decode_turn_ref(notice.identity)?,
                occurred_at: notice.occurred_at,
                notice: harness_pb::decode_system_notice(notice.notice.ok_or_else(invalid)?)?,
            })
        }
    };
    Ok(TimelineEvent {
        event_id: value.event_id,
        event_seq: value.event_seq,
        persisted_at: value.persisted_at,
        event,
    })
}

fn encode_stop_reason(value: AgentStopReason) -> i32 {
    match value {
        AgentStopReason::EndTurn => pb::StopReason::EndTurn as i32,
        AgentStopReason::OutputLimit => pb::StopReason::OutputLimit as i32,
        AgentStopReason::TransferUnresolved => pb::StopReason::TransferUnresolved as i32,
    }
}

fn decode_stop_reason(value: i32) -> TalosResult<AgentStopReason> {
    match pb::StopReason::try_from(value) {
        Ok(pb::StopReason::EndTurn) => Ok(AgentStopReason::EndTurn),
        Ok(pb::StopReason::OutputLimit) => Ok(AgentStopReason::OutputLimit),
        Ok(pb::StopReason::TransferUnresolved) => Ok(AgentStopReason::TransferUnresolved),
        _ => Err(invalid()),
    }
}

fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Agent Session data RPC payload is invalid",
    )
}

#[cfg(test)]
#[path = "resources_pb_tests.rs"]
mod tests;
