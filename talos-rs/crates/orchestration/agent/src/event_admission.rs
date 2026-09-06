use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageBlockContent,
    AgentMessageBlockStarted, AgentMessageBlockStatus, AgentMessageRole, AgentMessageStarted,
    AgentMessageStatus,
};
use talos_error::TalosError;

use crate::{AgentEventReducer, AgentResult};

/// Apply one completed User message as canonical lifecycle facts.
pub(crate) fn apply_user_message(
    reducer: &mut AgentEventReducer,
    message: AgentMessage,
) -> AgentResult<Vec<AgentEvent>> {
    validate_completed_user_message(&message)?;
    let mut facts = Vec::with_capacity(message.blocks.len().saturating_mul(2) + 2);
    let mut started_message = message.clone();
    started_message.status = AgentMessageStatus::InProgress;
    started_message.blocks.clear();
    started_message.completed_at = None;
    facts.push((
        message.created_at,
        AgentEventPayload::MessageStarted(AgentMessageStarted {
            message: started_message,
        }),
    ));
    for block in &message.blocks {
        let mut started_block = block.clone();
        started_block.status = AgentMessageBlockStatus::Running;
        started_block.completed_at = None;
        facts.push((
            block.started_at,
            AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: message.message_id.clone(),
                block: started_block,
            }),
        ));
        facts.push((
            block
                .completed_at
                .expect("validated completed User block has completed_at"),
            AgentEventPayload::MessageBlockCompleted(talos_agent_api::AgentMessageBlockCompleted {
                message_id: message.message_id.clone(),
                block: block.clone(),
            }),
        ));
    }
    facts.push((
        message
            .completed_at
            .expect("validated completed User message has completed_at"),
        AgentEventPayload::MessageCompleted(message),
    ));

    facts
        .into_iter()
        .map(|(occurred_at, payload)| {
            let event = AgentEvent {
                identity: reducer.identity().clone(),
                sequence: reducer.next_sequence(),
                occurred_at,
                payload,
            };
            reducer.apply(event.clone())?;
            Ok(event)
        })
        .collect()
}

fn validate_completed_user_message(message: &AgentMessage) -> AgentResult<()> {
    if message.role != AgentMessageRole::User
        || message.status != AgentMessageStatus::Completed
        || message.completed_at.is_none()
        || message.blocks.iter().any(|block| {
            block.status != AgentMessageBlockStatus::Completed
                || block.completed_at.is_none()
                || !matches!(block.content, AgentMessageBlockContent::UserContent(_))
        })
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Agent admission requires one fully completed User message",
        ));
    }
    Ok(())
}
