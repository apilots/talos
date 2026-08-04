use sha2::{Digest, Sha256};
use std::collections::HashSet;
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

fn validate_user_input(input: &[TalosInputItem]) -> TalosResult<()> {
    let has_text = input.iter().any(|item| match item {
        TalosInputItem::Text { text } => !text.trim().is_empty(),
        _ => false,
    });
    let files = input
        .iter()
        .filter_map(|item| match item {
            TalosInputItem::Files { file_ids } => Some(file_ids),
            _ => None,
        })
        .collect::<Vec<_>>();
    if files.len() > 1 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "input must include at most one files item".to_string(),
        ));
    }
    if let Some(file_ids) = files.first() {
        if file_ids.is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "files input must include at least one file id".to_string(),
            ));
        }
        let mut unique_ids = HashSet::with_capacity(file_ids.len());
        if file_ids
            .iter()
            .any(|file_id| file_id.trim().is_empty() || !unique_ids.insert(file_id))
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "file ids must be non-empty and unique".to_string(),
            ));
        }
    }
    if has_text || !files.is_empty() {
        Ok(())
    } else {
        Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "input must include non-empty text or files".to_string(),
        ))
    }
}

fn validate_retry_request(request: &TurnRetryParams) -> TalosResult<()> {
    if request.client_message_id.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "client_message_id is required".to_string(),
        ));
    }
    if request.retry_of_message_id.trim().is_empty() {
        return Err(TalosError::public(
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

fn result_from_idempotency_record(record: IdempotencyRecord) -> TalosResult<MessageCommandResult> {
    record.result.ok_or_else(|| {
        TalosError::internal(format!(
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
        if err.is_code(talos_error::ErrorCode::ResourceNotFound) {
            warn!(
                session_id = %session_id,
                "session was deleted before its generated title could be persisted"
            );
        } else {
            error!(
                session_id = %session_id,
                error = %err,
                "failed to persist session title"
            );
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
    if completed.stop_reason == talos_types::talos::StopReason::Cancelled {
        return;
    }
    let Some(artifact_seq) = completed.artifact_seq else {
        error!(
            session_id = %event.session_id,
            turn_id = %event.turn_id,
            "completed turn is missing its persisted artifact sequence"
        );
        return;
    };
    if let Err(err) = sessions
        .record_persisted_turn_progress(
            &event.session_id,
            artifact_seq,
            assistant_model,
            &completed.usage,
        )
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
    let bytes =
        serde_json::to_vec(&canonical).map_err(|err| TalosError::internal(err.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
