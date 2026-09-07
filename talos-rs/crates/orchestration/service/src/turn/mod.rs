use sha2::{Digest, Sha256};
use std::collections::HashSet;
use talos_agent_api::{AgentEvent, AgentEventPayload, AgentUserContent, AgentUserInput};
use talos_error::{TalosError, TalosResult};
use talos_store::session::IdempotencyRecord;
use talos_types::StartTurnResponse;

mod agent_execution;
mod agent_message;
mod service;
mod types;

pub(crate) use agent_execution::AgentExecutionDriver;
pub(crate) use agent_message::{completed_agent_user_input, completed_user_action_message};
pub(crate) use service::TurnService;
pub(crate) use types::{PreparedTurnSubmission, StartTurnReservation, TurnRetryParams};

fn validate_user_input(input: &AgentUserInput) -> TalosResult<()> {
    let has_text = input.content.iter().any(|item| match item {
        AgentUserContent::Text { text } => !text.trim().is_empty(),
        _ => false,
    });
    let file_ids = input
        .content
        .iter()
        .filter_map(|item| match item {
            AgentUserContent::File { file_id } => Some(file_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut unique_ids = HashSet::with_capacity(file_ids.len());
    if file_ids
        .iter()
        .any(|file_id| file_id.trim().is_empty() || !unique_ids.insert(file_id.as_str()))
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file ids must be non-empty and unique".to_string(),
        ));
    }
    if has_text || !file_ids.is_empty() {
        Ok(())
    } else {
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "input must include non-empty text or files".to_string(),
        ))
    }
}

fn validate_active_turn_input(input: &AgentUserInput) -> TalosResult<()> {
    validate_user_input(input)?;
    if input.content.iter().any(|item| {
        !matches!(
            item,
            AgentUserContent::Text { .. } | AgentUserContent::File { .. }
        )
    }) {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "active turn input supports only text and files",
        ));
    }
    Ok(())
}

fn validate_retry_request(request: &TurnRetryParams) -> TalosResult<()> {
    if request.client_message_id.trim().is_empty() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "client_message_id is required".to_string(),
        ));
    }
    if request.retry_of_message_id.trim().is_empty() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "retry_of_message_id is required".to_string(),
        ));
    }
    Ok(())
}

fn retry_idempotency_key(request: &TurnRetryParams) -> String {
    format!(
        "retry_{}_{}",
        request.client_message_id, request.retry_of_message_id
    )
}

fn start_response_from_idempotency_record(
    record: IdempotencyRecord,
) -> TalosResult<StartTurnResponse> {
    let status = record.outcome.ok_or_else(|| {
        TalosError::internal("The completed idempotency record is missing its outcome.").context(
            format!(
                "completed idempotency record is missing outcome: session_id={}, key={}",
                record.session_id, record.idempotency_key
            ),
        )
    })?;
    Ok(StartTurnResponse {
        request_id: record.request_id,
        message_id: record.message_id,
        session_id: record.session_id,
        turn_id: record.turn_id,
        status,
    })
}

fn auto_dispatch_session_from_event(event: &AgentEvent) -> Option<String> {
    match &event.payload {
        AgentEventPayload::TurnCompleted(_) => Some(event.identity.session_id.clone()),
        AgentEventPayload::TurnFailed(failed)
            if failed.error.code != talos_error::ErrorCode::Cancelled =>
        {
            Some(event.identity.session_id.clone())
        }
        AgentEventPayload::TurnFailed(_)
        | AgentEventPayload::WaitingForInput(_)
        | AgentEventPayload::TurnStarted(_)
        | AgentEventPayload::MessageStarted(_)
        | AgentEventPayload::MessageBlockStarted(_)
        | AgentEventPayload::MessageBlockDelta(_)
        | AgentEventPayload::MessageBlockCompleted(_)
        | AgentEventPayload::MessageCompleted(_)
        | AgentEventPayload::MetricsUpdated(_) => None,
    }
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;

pub(crate) fn turn_submission_hash(
    session_id: &str,
    input: &AgentUserInput,
) -> TalosResult<String> {
    let canonical = serde_json::json!({
        "session_id": session_id,
        "input": input,
    });
    let bytes = serde_json::to_vec(&canonical).map_err(|err| {
        TalosError::internal_with_source(
            "Failed to serialize the turn request; technical details were withheld.",
            err,
        )
    })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn start_turn_hash(request: &talos_types::StartTurnRequest) -> TalosResult<String> {
    let bytes = serde_json::to_vec(request).map_err(|error| {
        TalosError::internal_with_source(
            "Failed to serialize the turn request; technical details were withheld.",
            error,
        )
    })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
