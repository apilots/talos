use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use talos_auth::RequestIdentity;
use talos_types::{
    InterruptTurnRequest, InterruptTurnResponse, RetryTurnRequest, StartTurnRequest,
    SubmitDynamicToolResultRequest, SubmitDynamicToolResultResponse,
};

use super::access::shared_actor;
use super::errors::{ApiError, HttpApiErrorCode};
use super::state::HttpApiState;

pub(super) async fn start_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    headers: HeaderMap,
    Json(request): Json<StartTurnRequest>,
) -> Result<Response, ApiError> {
    let idempotency_key = start_turn_idempotency_key(&headers, request.client_request_id.clone())?;
    let ack = state
        .gateway
        .start_turn(&shared_actor(&actor.0), request, idempotency_key)
        .await
        .map_err(map_turn_submit_error)?;
    Ok((StatusCode::ACCEPTED, Json(ack)).into_response())
}

pub(super) async fn retry_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    _headers: HeaderMap,
    Json(request): Json<RetryTurnRequest>,
) -> Result<Response, ApiError> {
    let ack = state
        .gateway
        .retry_turn(&shared_actor(&actor.0), request)
        .await
        .map_err(map_retry_submit_error)?;
    Ok((StatusCode::ACCEPTED, Json(ack)).into_response())
}

pub(super) async fn interrupt_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<InterruptTurnRequest>,
) -> Result<Json<InterruptTurnResponse>, ApiError> {
    state
        .gateway
        .interrupt_turn(&shared_actor(&actor.0), request)
        .await
        .map(Json)
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                HttpApiErrorCode::InvalidRequest,
                "session not found",
            ),
            _ => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                HttpApiErrorCode::InternalError,
                err.to_string(),
            ),
        })
}

fn start_turn_idempotency_key(
    headers: &HeaderMap,
    client_request_id: Option<String>,
) -> Result<Option<String>, ApiError> {
    let header_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if let (Some(header), Some(body)) = (&header_key, &client_request_id) {
        if header != body {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                HttpApiErrorCode::InvalidRequest,
                "idempotency-key header must match client_request_id",
            ));
        }
    }
    Ok(header_key.or(client_request_id))
}

pub(super) async fn submit_dynamic_tool_result_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<SubmitDynamicToolResultRequest>,
) -> Result<Json<SubmitDynamicToolResultResponse>, ApiError> {
    state
        .gateway
        .submit_dynamic_tool_result(&shared_actor(&actor.0), request)
        .await
        .map(Json)
        .map_err(map_turn_submit_error)
}

fn map_retry_submit_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ValidationError(message) => ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidRetryRequest,
            message,
        ),
        talos_error::TalosError::RetryActiveTurnRunning(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::RetryActiveTurnRunning,
            message,
        ),
        talos_error::TalosError::RetrySourceCompleted(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::RetrySourceCompleted,
            message,
        ),
        talos_error::TalosError::RetrySourceMismatch(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::RetrySourceMismatch,
            message,
        ),
        talos_error::TalosError::IdempotencyConflict(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::IdempotencyConflict,
            message,
        ),
        other => map_turn_submit_error(other),
    }
}

fn map_turn_submit_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            HttpApiErrorCode::InvalidRequest,
            "session not found",
        ),
        talos_error::TalosError::ValidationError(message) => ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        talos_error::TalosError::IdempotencyConflict(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::IdempotencyConflict,
            message,
        ),
        talos_error::TalosError::ActiveTurnRunning(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::ActiveTurnRunning,
            message,
        ),
        talos_error::TalosError::Conflict(message) => {
            ApiError::new(StatusCode::CONFLICT, HttpApiErrorCode::Conflict, message)
        }
        talos_error::TalosError::SerializationError(message) => ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        talos_error::TalosError::Timeout(message) => ApiError::new(
            StatusCode::GATEWAY_TIMEOUT,
            HttpApiErrorCode::FirstEventTimeout,
            message,
        ),
        talos_error::TalosError::StorageError(message) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::EventRecordFailed,
            message,
        ),
        talos_error::TalosError::IdempotencyError(message) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::IdempotencyFailed,
            message,
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::InternalError,
            other.to_string(),
        ),
    }
}
