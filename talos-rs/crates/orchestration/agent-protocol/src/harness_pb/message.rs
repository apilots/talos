use agent_pb as pb;
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentExecutionSummary, AgentMessage,
    AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus, AgentMessageRole,
    AgentMessageStatus, AgentReasoning, AgentToolCall, AgentToolCallArguments, AgentToolProgress,
    AgentToolResult,
};
use talos_error::TalosResult;
use talos_types::{TalosToolProgressKind, ToolArguments};

use super::compaction::{decode_compaction, encode_compaction};
use super::input::{decode_user_content, encode_user_content};
use super::invalid;
use super::notice::{decode_system_notice, encode_system_notice};
use super::tool_content::{decode_tool_content, encode_tool_content};

pub fn encode_message(value: AgentMessage) -> TalosResult<pb::AgentMessage> {
    Ok(pb::AgentMessage {
        message_id: value.message_id,
        turn_id: value.turn_id,
        role: encode_role(value.role),
        status: encode_status(value.status),
        blocks: value
            .blocks
            .into_iter()
            .map(encode_block)
            .collect::<TalosResult<_>>()?,
        created_at: value.created_at,
        completed_at: value.completed_at,
    })
}

pub fn decode_message(value: pb::AgentMessage) -> TalosResult<AgentMessage> {
    Ok(AgentMessage {
        message_id: value.message_id,
        turn_id: value.turn_id,
        role: decode_role(value.role)?,
        status: decode_status(value.status)?,
        blocks: value
            .blocks
            .into_iter()
            .map(decode_block)
            .collect::<TalosResult<_>>()?,
        created_at: value.created_at,
        completed_at: value.completed_at,
    })
}

pub fn encode_block(value: AgentMessageBlock) -> TalosResult<pb::MessageBlock> {
    use pb::message_block::Content;
    Ok(pb::MessageBlock {
        block_id: value.block_id,
        status: encode_block_status(value.status),
        started_at: value.started_at,
        completed_at: value.completed_at,
        content: Some(match value.content {
            AgentMessageBlockContent::UserContent(value) => {
                Content::UserContent(encode_user_content(value))
            }
            AgentMessageBlockContent::Reasoning(value) => Content::Reasoning(value.text),
            AgentMessageBlockContent::AssistantText(value) => {
                Content::AssistantText(pb::AssistantText {
                    phase: encode_phase(value.phase),
                    text: value.text,
                })
            }
            AgentMessageBlockContent::ToolCall(value) => {
                Content::ToolCall(encode_tool_call(value)?)
            }
            AgentMessageBlockContent::ToolResult(value) => Content::ToolResult(pb::ToolResult {
                call_id: value.call_id,
                tool_name: value.tool_name,
                content: value
                    .content
                    .into_iter()
                    .map(encode_tool_content)
                    .collect::<TalosResult<_>>()?,
                is_error: value.is_error,
            }),
            AgentMessageBlockContent::ExecutionSummary(value) => {
                Content::ExecutionSummary(value.text)
            }
            AgentMessageBlockContent::ContextCompaction(value) => {
                Content::ContextCompaction(encode_compaction(value))
            }
            AgentMessageBlockContent::SystemNotice(value) => {
                Content::SystemNotice(encode_system_notice(value))
            }
        }),
    })
}

pub fn decode_block(value: pb::MessageBlock) -> TalosResult<AgentMessageBlock> {
    use pb::message_block::Content;
    let content = match value.content.ok_or_else(invalid)? {
        Content::UserContent(value) => {
            AgentMessageBlockContent::UserContent(decode_user_content(value)?)
        }
        Content::Reasoning(text) => AgentMessageBlockContent::Reasoning(AgentReasoning { text }),
        Content::AssistantText(value) => {
            AgentMessageBlockContent::AssistantText(AgentAssistantText {
                phase: decode_phase(value.phase)?,
                text: value.text,
            })
        }
        Content::ToolCall(value) => AgentMessageBlockContent::ToolCall(decode_tool_call(value)?),
        Content::ToolResult(value) => AgentMessageBlockContent::ToolResult(AgentToolResult {
            call_id: value.call_id,
            tool_name: value.tool_name,
            content: value
                .content
                .into_iter()
                .map(decode_tool_content)
                .collect::<TalosResult<_>>()?,
            is_error: value.is_error,
        }),
        Content::ExecutionSummary(text) => {
            AgentMessageBlockContent::ExecutionSummary(AgentExecutionSummary { text })
        }
        Content::ContextCompaction(value) => {
            AgentMessageBlockContent::ContextCompaction(decode_compaction(value)?)
        }
        Content::SystemNotice(value) => {
            AgentMessageBlockContent::SystemNotice(decode_system_notice(value)?)
        }
    };
    Ok(AgentMessageBlock {
        block_id: value.block_id,
        status: decode_block_status(value.status)?,
        content,
        started_at: value.started_at,
        completed_at: value.completed_at,
    })
}

pub fn encode_tool_progress(value: AgentToolProgress) -> pb::ToolProgress {
    pb::ToolProgress {
        kind: match value.kind {
            TalosToolProgressKind::Status => pb::ProgressKind::Status as i32,
            TalosToolProgressKind::Stdout => pb::ProgressKind::Stdout as i32,
            TalosToolProgressKind::Stderr => pb::ProgressKind::Stderr as i32,
        },
        text: value.text,
        occurred_at: value.occurred_at,
    }
}

pub fn decode_tool_progress(value: pb::ToolProgress) -> TalosResult<AgentToolProgress> {
    let kind = match pb::ProgressKind::try_from(value.kind) {
        Ok(pb::ProgressKind::Status) => TalosToolProgressKind::Status,
        Ok(pb::ProgressKind::Stdout) => TalosToolProgressKind::Stdout,
        Ok(pb::ProgressKind::Stderr) => TalosToolProgressKind::Stderr,
        _ => return Err(invalid()),
    };
    Ok(AgentToolProgress {
        kind,
        text: value.text,
        occurred_at: value.occurred_at,
    })
}

fn encode_tool_call(value: AgentToolCall) -> TalosResult<pb::ToolCall> {
    Ok(pb::ToolCall {
        call_id: value.call_id,
        name: value.name,
        arguments: Some(match value.arguments {
            AgentToolCallArguments::Streaming { json_fragment } => {
                pb::tool_call::Arguments::JsonFragment(json_fragment)
            }
            AgentToolCallArguments::Complete { value } => {
                pb::tool_call::Arguments::Complete(pb::ToolArguments {
                    json_utf8: serde_json::to_string(value.as_inner()).map_err(|_| invalid())?,
                })
            }
        }),
        progress: value
            .progress
            .into_iter()
            .map(encode_tool_progress)
            .collect(),
    })
}

fn decode_tool_call(value: pb::ToolCall) -> TalosResult<AgentToolCall> {
    let arguments = match value.arguments.ok_or_else(invalid)? {
        pb::tool_call::Arguments::JsonFragment(json_fragment) => {
            AgentToolCallArguments::Streaming { json_fragment }
        }
        pb::tool_call::Arguments::Complete(value) => AgentToolCallArguments::Complete {
            value: ToolArguments::new(
                serde_json::from_str(&value.json_utf8).map_err(|_| invalid())?,
            ),
        },
    };
    Ok(AgentToolCall {
        call_id: value.call_id,
        name: value.name,
        arguments,
        progress: value
            .progress
            .into_iter()
            .map(decode_tool_progress)
            .collect::<TalosResult<_>>()?,
    })
}

pub fn encode_phase(value: AgentAssistantTextPhase) -> i32 {
    match value {
        AgentAssistantTextPhase::Pending => pb::AssistantPhase::Pending as i32,
        AgentAssistantTextPhase::Commentary => pb::AssistantPhase::Commentary as i32,
        AgentAssistantTextPhase::FinalAnswer => pb::AssistantPhase::FinalAnswer as i32,
    }
}

pub fn decode_phase(value: i32) -> TalosResult<AgentAssistantTextPhase> {
    match pb::AssistantPhase::try_from(value) {
        Ok(pb::AssistantPhase::Pending) => Ok(AgentAssistantTextPhase::Pending),
        Ok(pb::AssistantPhase::Commentary) => Ok(AgentAssistantTextPhase::Commentary),
        Ok(pb::AssistantPhase::FinalAnswer) => Ok(AgentAssistantTextPhase::FinalAnswer),
        _ => Err(invalid()),
    }
}

fn encode_role(value: AgentMessageRole) -> i32 {
    match value {
        AgentMessageRole::User => pb::MessageRole::User as i32,
        AgentMessageRole::Agent => pb::MessageRole::Agent as i32,
        AgentMessageRole::Tool => pb::MessageRole::Tool as i32,
        AgentMessageRole::System => pb::MessageRole::System as i32,
    }
}

fn decode_role(value: i32) -> TalosResult<AgentMessageRole> {
    match pb::MessageRole::try_from(value) {
        Ok(pb::MessageRole::User) => Ok(AgentMessageRole::User),
        Ok(pb::MessageRole::Agent) => Ok(AgentMessageRole::Agent),
        Ok(pb::MessageRole::Tool) => Ok(AgentMessageRole::Tool),
        Ok(pb::MessageRole::System) => Ok(AgentMessageRole::System),
        _ => Err(invalid()),
    }
}

fn encode_status(value: AgentMessageStatus) -> i32 {
    match value {
        AgentMessageStatus::InProgress => pb::MessageStatus::InProgress as i32,
        AgentMessageStatus::Completed => pb::MessageStatus::Completed as i32,
        AgentMessageStatus::Failed => pb::MessageStatus::Failed as i32,
        AgentMessageStatus::Cancelled => pb::MessageStatus::Cancelled as i32,
    }
}

fn decode_status(value: i32) -> TalosResult<AgentMessageStatus> {
    match pb::MessageStatus::try_from(value) {
        Ok(pb::MessageStatus::InProgress) => Ok(AgentMessageStatus::InProgress),
        Ok(pb::MessageStatus::Completed) => Ok(AgentMessageStatus::Completed),
        Ok(pb::MessageStatus::Failed) => Ok(AgentMessageStatus::Failed),
        Ok(pb::MessageStatus::Cancelled) => Ok(AgentMessageStatus::Cancelled),
        _ => Err(invalid()),
    }
}

fn encode_block_status(value: AgentMessageBlockStatus) -> i32 {
    match value {
        AgentMessageBlockStatus::Queued => pb::BlockStatus::Queued as i32,
        AgentMessageBlockStatus::Running => pb::BlockStatus::Running as i32,
        AgentMessageBlockStatus::WaitingForInput => pb::BlockStatus::WaitingForInput as i32,
        AgentMessageBlockStatus::Completed => pb::BlockStatus::Completed as i32,
        AgentMessageBlockStatus::Failed => pb::BlockStatus::Failed as i32,
        AgentMessageBlockStatus::Cancelled => pb::BlockStatus::Cancelled as i32,
    }
}

fn decode_block_status(value: i32) -> TalosResult<AgentMessageBlockStatus> {
    match pb::BlockStatus::try_from(value) {
        Ok(pb::BlockStatus::Queued) => Ok(AgentMessageBlockStatus::Queued),
        Ok(pb::BlockStatus::Running) => Ok(AgentMessageBlockStatus::Running),
        Ok(pb::BlockStatus::WaitingForInput) => Ok(AgentMessageBlockStatus::WaitingForInput),
        Ok(pb::BlockStatus::Completed) => Ok(AgentMessageBlockStatus::Completed),
        Ok(pb::BlockStatus::Failed) => Ok(AgentMessageBlockStatus::Failed),
        Ok(pb::BlockStatus::Cancelled) => Ok(AgentMessageBlockStatus::Cancelled),
        _ => Err(invalid()),
    }
}
