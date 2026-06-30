use std::sync::Arc;

use sha2::{Digest, Sha256};
use talos_coordinator::SessionRuntime;
use talos_error::{TalosError, TalosResult};
use talos_store::session::{IdempotencyRecord, MessageCommandResult};
use talos_types::talos::{TalosInputItem, TalosTimelineEvent, TalosTimelineEventPayload};
use tokio::sync::mpsc;
use tracing::error;

use crate::session::SessionRecordService;
use crate::timeline::TimelineEventService;

mod service;
mod types;

pub use service::TurnService;
pub use types::{AgentDispatchTurnExecutor, TurnExecutor, TurnRetryRequest};

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

fn validate_retry_request(request: &TurnRetryRequest) -> TalosResult<()> {
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

fn retry_idempotency_key(request: &TurnRetryRequest) -> String {
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

fn spawn_event_forwarder(
    event_service: Arc<TimelineEventService>,
    turn_executor: Arc<dyn TurnExecutor>,
    session_records: Arc<SessionRecordService>,
    session_runtime: Arc<dyn SessionRuntime>,
    assistant_model: String,
    initial_completed_turn: Option<(String, String)>,
    mut stream_rx: mpsc::Receiver<TalosTimelineEvent>,
) {
    tokio::spawn(async move {
        let mut completed_turn = initial_completed_turn;
        while let Some(event) = stream_rx.recv().await {
            let progress_event = event.clone();
            if let Err(err) = event_service.record_talos_event(event).await {
                error!(error = %err, "failed to persist timeline stream event");
                continue;
            }
            record_completed_turn_progress(
                session_records.as_ref(),
                session_runtime.as_ref(),
                &progress_event,
                &assistant_model,
            )
            .await;
            if let Some(turn) = completed_turn_from_event(&progress_event) {
                completed_turn = Some(turn);
            }
        }

        if let Some((session_id, turn_id)) = completed_turn {
            if let Some(title) = turn_executor
                .take_session_title_update(&session_id, &turn_id)
                .await
            {
                record_session_title(
                    session_records.as_ref(),
                    session_runtime.as_ref(),
                    &session_id,
                    &title,
                )
                .await;
            }
        }
    });
}

fn completed_turn_from_event(event: &TalosTimelineEvent) -> Option<(String, String)> {
    matches!(event.payload, TalosTimelineEventPayload::TurnCompleted(_))
        .then(|| (event.session_id.clone(), event.turn_id.clone()))
}

async fn record_session_title(
    session_records: &SessionRecordService,
    session_runtime: &dyn SessionRuntime,
    session_id: &str,
    title: &str,
) {
    if let Err(err) = session_records
        .update_session_title(session_id, title)
        .await
    {
        error!(
            session_id = %session_id,
            error = %err,
            "failed to persist session title"
        );
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
    session_records: &SessionRecordService,
    session_runtime: &dyn SessionRuntime,
    event: &TalosTimelineEvent,
    assistant_model: &str,
) {
    let TalosTimelineEventPayload::TurnCompleted(completed) = &event.payload else {
        return;
    };
    if let Err(err) = session_records
        .record_session_progress(
            &event.session_id,
            2,
            &completed.usage,
            Some(assistant_model),
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
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|err| TalosError::SerializationError(err.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
