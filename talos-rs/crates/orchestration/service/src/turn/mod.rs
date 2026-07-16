use sha2::{Digest, Sha256};
use talos_coordinator::SessionRuntime;
use talos_error::{TalosError, TalosResult};
use talos_store::session::{IdempotencyRecord, MessageCommandResult};
use talos_types::talos::{AgentEvent, AgentEventPayload, TalosInputItem};
use tracing::{error, warn};

use crate::session::SessionService;

mod service;
mod types;

pub use service::TurnService;
pub use types::{AgentDispatchTurnExecutor, TurnExecutor, TurnRetryParams};

fn validate_text_input(input: &[TalosInputItem]) -> TalosResult<()> {
    let has_text = input.iter().any(|item| match item {
        TalosInputItem::Text { text } => !text.trim().is_empty(),
        _ => false,
    });
    if has_text {
        return Ok(());
    }
    Err(TalosError::ValidationError(
        "input must include at least one non-empty text item".to_string(),
    ))
}

fn validate_retry_request(request: &TurnRetryParams) -> TalosResult<()> {
    if request.client_message_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "client_message_id is required".to_string(),
        ));
    }
    if request.retry_of_message_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
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

fn result_from_idempotency_record(record: IdempotencyRecord) -> TalosResult<MessageCommandResult> {
    record.result.ok_or_else(|| {
        TalosError::IdempotencyError(format!(
            "idempotency record is missing result: session_id={}, key={}, status={}",
            record.session_id,
            record.idempotency_key,
            record.status.as_str()
        ))
    })
}

fn completed_turn_from_event(event: &AgentEvent) -> Option<(String, String)> {
    matches!(event.payload, AgentEventPayload::TurnCompleted(_))
        .then(|| (event.session_id.clone(), event.turn_id.clone()))
}

async fn record_session_title(
    sessions: &SessionService,
    session_runtime: &dyn SessionRuntime,
    session_id: &str,
    title: &str,
) {
    if let Err(err) = sessions.update_session_title(session_id, title).await {
        match err {
            TalosError::ResourceNotFound(_) => warn!(
                session_id = %session_id,
                "session was deleted before its generated title could be persisted"
            ),
            err => error!(
                session_id = %session_id,
                error = %err,
                "failed to persist session title"
            ),
        }
        return;
    }
    if let Err(err) = session_runtime.invalidate_cached_session(session_id).await {
        error!(
            session_id = %session_id,
            error = %err,
            "failed to invalidate session cache after title update"
        );
    }
}

async fn record_completed_turn_progress(
    sessions: &SessionService,
    session_runtime: &dyn SessionRuntime,
    event: &AgentEvent,
    assistant_model: &str,
) {
    let AgentEventPayload::TurnCompleted(completed) = &event.payload else {
        return;
    };
    if let Err(err) = sessions
        .record_turn_progress(&event.session_id, 2, assistant_model, &completed.usage)
        .await
    {
        error!(
            session_id = %event.session_id,
            turn_id = %event.turn_id,
            error = %err,
            "failed to persist completed turn session progress"
        );
        return;
    }
    if let Err(err) = session_runtime
        .invalidate_cached_session(&event.session_id)
        .await
    {
        error!(
            session_id = %event.session_id,
            turn_id = %event.turn_id,
            error = %err,
            "failed to invalidate session cache after turn progress"
        );
    }
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;

pub fn turn_submission_hash(
    session_id: &str,
    input: &[TalosInputItem],
    workspace_id: &Option<String>,
) -> TalosResult<String> {
    let canonical = serde_json::json!({
        "session_id": session_id,
        "workspace_id": workspace_id,
        "input": input,
    });
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|err| TalosError::SerializationError(err.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
