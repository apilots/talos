use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Json, Response};
use talos_agent_api::{
    InterruptTurnRequest, InterruptTurnResponse, SubmitTurnInputRequest, SubmitTurnInputResponse,
};
use talos_auth::RequestIdentity;
use talos_types::{RetryTurnRequest, StartTurnRequest};

use super::access::shared_actor;
use super::errors::{api_success, api_success_with_status, ApiError, JsonResponse};
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
        .start_turn(
            &shared_actor(&actor.0),
            talos_types::talos::SessionSource::http(),
            request,
            idempotency_key,
        )
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success_with_status(StatusCode::ACCEPTED, ack))
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
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success_with_status(StatusCode::ACCEPTED, ack))
}

pub(super) async fn interrupt_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<InterruptTurnRequest>,
) -> Result<JsonResponse<InterruptTurnResponse>, ApiError> {
    state
        .gateway
        .interrupt_turn(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
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
            return Err(ApiError::new(talos_error::ErrorCode::InvalidRequest));
        }
    }
    Ok(header_key.or(client_request_id))
}

pub(super) async fn submit_turn_input_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<SubmitTurnInputRequest>,
) -> Result<JsonResponse<SubmitTurnInputResponse>, ApiError> {
    state
        .gateway
        .submit_turn_input(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
