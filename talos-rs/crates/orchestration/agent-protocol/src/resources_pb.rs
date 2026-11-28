//! Typed projections for the control-plane data read by a remote Harness.

use agent_pb as pb;
use std::str::FromStr;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    AgentCaller, AgentPath, AgentTarget, AgentTreeSnapshot, CollaborationToolResult, FileInfo,
    FileScope, FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest,
    SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest, SubagentSnapshot,
    SubagentSpawnResult, WaitSubagentRequest,
};

use crate::harness_pb;

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
    Ok(pb::ToolFileInfo {
        file_id: value.file_id,
        original_filename: value.original_filename,
        mime_type: value.mime_type,
        size_bytes: value.size_bytes,
        tags: value.tags,
        metadata: value.metadata.into_iter().collect(),
        created_at: value.created_at,
        updated_at: value.updated_at,
        scope: Some(pb::FileScope {
            scope: Some(match value.scope {
                FileScope::Personal => pb::file_scope::Scope::Personal(pb::Empty {}),
                FileScope::Project { project_id } => pb::file_scope::Scope::ProjectId(project_id),
            }),
        }),
    })
}

pub fn decode_tool_file(value: pb::ToolFileInfo) -> TalosResult<FileInfo> {
    if value.file_id.is_empty() || value.original_filename.is_empty() || value.mime_type.is_empty()
    {
        return Err(invalid());
    }
    Ok(FileInfo {
        file_id: value.file_id,
        scope: match value
            .scope
            .and_then(|scope| scope.scope)
            .ok_or_else(invalid)?
        {
            pb::file_scope::Scope::Personal(_) => FileScope::Personal,
            pb::file_scope::Scope::ProjectId(project_id) if !project_id.is_empty() => {
                FileScope::Project { project_id }
            }
            _ => return Err(invalid()),
        },
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

fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Agent Session data RPC payload is invalid",
    )
}
