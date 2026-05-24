use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;

use talos_application_service::TurnRetryRequest;
use talos_auth::RequestIdentity;
use talos_session_store::MessageCommandAck;
use talos_types::talos::TalosMessageRequest;

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn session_message_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<TalosMessageRequest>,
) -> Result<Response, ApiError> {
    let ack = submit_message_command(state, actor.0, headers, session_id, request).await?;
    Ok((StatusCode::ACCEPTED, Json(ack)).into_response())
}

#[derive(Deserialize)]
pub(super) struct MessageRetryRequest {
    input: Vec<talos_types::talos::TalosInputItem>,
    workspace_id: Option<String>,
    client_message_id: String,
    retry_of_message_id: String,
}

pub(super) async fn retry_message_attempt_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    _headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<MessageRetryRequest>,
) -> Result<Response, ApiError> {
    let ack = state
        .app
        .submit_retry(
            &shared_actor(&actor.0),
            &session_id,
            TurnRetryRequest {
                input: request.input,
                workspace_id: request.workspace_id,
                client_message_id: request.client_message_id,
                retry_of_message_id: request.retry_of_message_id,
            },
        )
        .await
        .map_err(map_retry_submit_error)?;
    Ok((StatusCode::ACCEPTED, Json(ack)).into_response())
}

pub(super) async fn submit_message_command(
    state: Arc<HttpApiState>,
    actor: RequestIdentity,
    headers: HeaderMap,
    session_id: String,
    request: TalosMessageRequest,
) -> Result<MessageCommandAck, ApiError> {
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    state
        .app
        .submit_message(&shared_actor(&actor), &session_id, request, idempotency_key)
        .await
        .map_err(map_turn_submit_error)
}

#[derive(Deserialize)]
pub(super) struct DynamicToolResultRequest {
    call_id: String,
    tool_name: String,
    output: String,
    success: bool,
}

pub(super) async fn submit_dynamic_tool_result_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    Json(body): Json<DynamicToolResultRequest>,
) -> impl IntoResponse {
    match state
        .app
        .submit_dynamic_tool_result(
            &shared_actor(&actor.0),
            &session_id,
            &body.call_id,
            &body.tool_name,
            body.output,
            body.success,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_turn_submit_error(e).into_response(),
    }
}

fn map_retry_submit_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_retry_request", message)
        }
        talos_error::TalosError::RetryActiveTurnRunning(message) => {
            ApiError::new(StatusCode::CONFLICT, "retry_active_turn_running", message)
        }
        talos_error::TalosError::RetrySourceCompleted(message) => {
            ApiError::new(StatusCode::CONFLICT, "retry_source_completed", message)
        }
        talos_error::TalosError::RetrySourceMismatch(message) => {
            ApiError::new(StatusCode::CONFLICT, "retry_source_mismatch", message)
        }
        talos_error::TalosError::IdempotencyConflict(message) => {
            ApiError::new(StatusCode::CONFLICT, "idempotency_conflict", message)
        }
        other => map_turn_submit_error(other),
    }
}

fn map_turn_submit_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "session not found",
        ),
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        talos_error::TalosError::IdempotencyConflict(message) => {
            ApiError::new(StatusCode::CONFLICT, "idempotency_conflict", message)
        }
        talos_error::TalosError::Conflict(message) => {
            ApiError::new(StatusCode::CONFLICT, "active_turn_running", message)
        }
        talos_error::TalosError::SerializationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        talos_error::TalosError::Timeout(message) => {
            ApiError::new(StatusCode::GATEWAY_TIMEOUT, "first_event_timeout", message)
        }
        talos_error::TalosError::StorageError(message) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "event_record_failed",
            message,
        ),
        talos_error::TalosError::IdempotencyError(message) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "idempotency_failed",
            message,
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}
