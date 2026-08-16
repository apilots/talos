use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Json, Response};
use talos_auth::RequestIdentity;
use talos_types::{
    InterruptTurnRequest, InterruptTurnResponse, RetryTurnRequest, StartTurnRequest,
    SubmitDynamicToolResultRequest, SubmitDynamicToolResultResponse, SubmitUserActionRequest,
    SubmitUserActionResponse,
};

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
        .start_turn(&shared_actor(&actor.0), request, idempotency_key)
        .await
        .map_err(map_turn_submit_error)?;
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
        .map_err(map_retry_submit_error)?;
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
        .map_err(map_interrupt_turn_error)
}

fn map_interrupt_turn_error(error: talos_error::TalosError) -> ApiError {
    match error.error_code() {
        talos_error::ErrorCode::ResourceNotFound => {
            ApiError::new(talos_error::ErrorCode::SessionNotFound)
        }
        _ => ApiError::from_talos_error(error),
    }
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

pub(super) async fn submit_dynamic_tool_result_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<SubmitDynamicToolResultRequest>,
) -> Result<JsonResponse<SubmitDynamicToolResultResponse>, ApiError> {
    state
        .gateway
        .submit_dynamic_tool_result(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(map_turn_submit_error)
}

pub(super) async fn submit_user_action_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<SubmitUserActionRequest>,
) -> Result<JsonResponse<SubmitUserActionResponse>, ApiError> {
    state
        .gateway
        .submit_user_action(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(map_turn_submit_error)
}

fn map_retry_submit_error(error: talos_error::TalosError) -> ApiError {
    match error.error_code() {
        talos_error::ErrorCode::InvalidRequest => {
            ApiError::new(talos_error::ErrorCode::InvalidRetryRequest)
        }
        _ => map_turn_submit_error(error),
    }
}

fn map_turn_submit_error(error: talos_error::TalosError) -> ApiError {
    match error.error_code() {
        talos_error::ErrorCode::ResourceNotFound => {
            ApiError::new(talos_error::ErrorCode::SessionNotFound)
        }
        _ => ApiError::from_talos_error(error),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::Value;
    use talos_error::TalosError;

    use super::map_interrupt_turn_error;

    const ERROR_BODY_LIMIT: usize = 4096;

    #[tokio::test]
    async fn interrupt_error_maps_missing_session_to_session_not_found() {
        let response = map_interrupt_turn_error(TalosError::public(
            talos_error::ErrorCode::ResourceNotFound,
            "session missing-session".to_string(),
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), ERROR_BODY_LIMIT)
            .await
            .expect("error body should be readable");
        let payload: Value = serde_json::from_slice(&body).expect("error body should be json");
        assert_eq!(payload["status"], "error");
        assert_eq!(payload["error"]["code"], "sessionNotFound");
        assert_eq!(payload["error"]["message"], "Session not found");
    }

    #[tokio::test]
    async fn interrupt_error_uses_route_owned_session_not_found_semantics() {
        let response = map_interrupt_turn_error(TalosError::public(
            talos_error::ErrorCode::ResourceNotFound,
            "active turn for session mock-session".to_string(),
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), ERROR_BODY_LIMIT)
            .await
            .expect("error body should be readable");
        let payload: Value = serde_json::from_slice(&body).expect("error body should be json");
        assert_eq!(payload["status"], "error");
        assert_eq!(payload["error"]["code"], "sessionNotFound");
        assert_eq!(payload["error"]["message"], "Session not found");
    }
}
