use talos_agent_api::{
    AgentMessage, AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus,
    AgentMessageRole, AgentMessageStatus, AgentUserContent, AgentUserInput,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::UserActionResponse;

/// Builds one canonical User message from the public Agent input contract.
pub(crate) fn completed_agent_user_input(
    turn_id: &str,
    message_id: &str,
    input: AgentUserInput,
    occurred_at: i64,
) -> TalosResult<AgentMessage> {
    completed_message(turn_id, message_id, input.content, occurred_at)
}

/// Builds the canonical User message used to claim one waiting continuation.
pub(crate) fn completed_user_action_message(
    turn_id: &str,
    message_id: &str,
    action_id: &str,
    response: UserActionResponse,
    occurred_at: i64,
) -> TalosResult<AgentMessage> {
    completed_message(
        turn_id,
        message_id,
        vec![AgentUserContent::UserActionResponse {
            action_id: action_id.to_string(),
            response,
        }],
        occurred_at,
    )
}

fn completed_message(
    turn_id: &str,
    message_id: &str,
    content: Vec<AgentUserContent>,
    occurred_at: i64,
) -> TalosResult<AgentMessage> {
    if turn_id.trim().is_empty() || message_id.trim().is_empty() || content.is_empty() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "canonical Agent User message requires identity and content",
        ));
    }
    let blocks = content
        .into_iter()
        .enumerate()
        .map(|(index, content)| AgentMessageBlock {
            block_id: format!("{message_id}_block_{index}"),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::UserContent(content),
            started_at: occurred_at,
            completed_at: Some(occurred_at),
        })
        .collect();
    Ok(AgentMessage {
        message_id: message_id.to_string(),
        turn_id: turn_id.to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks,
        created_at: occurred_at,
        completed_at: Some(occurred_at),
    })
}

#[cfg(test)]
#[path = "agent_message_tests.rs"]
mod tests;
