use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_types::{
    SessionClearRequest, SessionCloseRequest, SessionCloseResponse, SessionCompactRequest,
    SessionDeleteRequest, SessionGetRequest, SessionHistoryRequest, SessionHistoryResponse,
    SessionListRequest, SessionPatchRequest,
};

use super::access::shared_actor;
use super::errors::{ApiError, HttpApiErrorCode};
use super::state::HttpApiState;

pub(super) async fn list_sessions_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Query(request): Query<SessionListRequest>,
) -> Result<Json<Value>, ApiError> {
    let response = state
        .gateway
        .list_sessions(&shared_actor(&actor.0), request)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                HttpApiErrorCode::InternalError,
                e.to_string(),
            )
        })?;
    Ok(Json(json!(response)))
}

pub(super) async fn get_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .gateway
        .get_session(&shared_actor(&actor.0), SessionGetRequest { session_id })
        .await
        .map_err(|e| map_session_error(e, "session not found"))?;

    Ok(Json(json!(session)))
}

pub(super) async fn get_session_history_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionHistoryResponse>, ApiError> {
    let response = state
        .gateway
        .get_session_history(
            &shared_actor(&actor.0),
            SessionHistoryRequest { session_id },
        )
        .await
        .map_err(|e| map_session_error(e, "session not found"))?;

    Ok(Json(response))
}

pub(super) async fn patch_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    Json(mut request): Json<SessionPatchRequest>,
) -> Result<Json<Value>, ApiError> {
    request.session_id = session_id;
    let response = state
        .gateway
        .patch_session(&shared_actor(&actor.0), request)
        .await
        .map_err(|e| map_patch_session_error(e, "session not found"))?;

    Ok(Json(json!(response)))
}

pub(super) async fn close_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionCloseResponse>, ApiError> {
    let response = state
        .gateway
        .close_session(&shared_actor(&actor.0), SessionCloseRequest { session_id })
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                HttpApiErrorCode::InvalidRequest,
                "session not found",
            ),
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                HttpApiErrorCode::InternalError,
                other.to_string(),
            ),
        })?;
    Ok(Json(response))
}

pub(super) async fn delete_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let response = state
        .gateway
        .delete_session(&shared_actor(&actor.0), SessionDeleteRequest { session_id })
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                HttpApiErrorCode::InvalidRequest,
                "session not found",
            ),
            talos_error::TalosError::ValidationError(message) => ApiError::new(
                StatusCode::CONFLICT,
                HttpApiErrorCode::InvalidRequest,
                message,
            ),
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                HttpApiErrorCode::InternalError,
                other.to_string(),
            ),
        })?;
    Ok(Json(json!(response)))
}

pub(super) async fn compact_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .gateway
        .compact_session(
            &shared_actor(&actor.0),
            SessionCompactRequest { session_id },
        )
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                HttpApiErrorCode::InvalidRequest,
                "session not found",
            ),
            talos_error::TalosError::ValidationError(message) => ApiError::new(
                StatusCode::CONFLICT,
                HttpApiErrorCode::InvalidRequest,
                message,
            ),
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                HttpApiErrorCode::InternalError,
                other.to_string(),
            ),
        })?;
    Ok(Json(json!(session)))
}

pub(super) async fn clear_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .gateway
        .clear_session(&shared_actor(&actor.0), SessionClearRequest { session_id })
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                HttpApiErrorCode::InvalidRequest,
                "session not found",
            ),
            talos_error::TalosError::ValidationError(message) => ApiError::new(
                StatusCode::CONFLICT,
                HttpApiErrorCode::InvalidRequest,
                message,
            ),
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                HttpApiErrorCode::InternalError,
                other.to_string(),
            ),
        })?;
    Ok(Json(json!(session)))
}

fn map_session_error(error: talos_error::TalosError, not_found_message: &str) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            HttpApiErrorCode::InvalidRequest,
            not_found_message,
        ),
        talos_error::TalosError::ValidationError(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::InternalError,
            other.to_string(),
        ),
    }
}

fn map_patch_session_error(error: talos_error::TalosError, not_found_message: &str) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            HttpApiErrorCode::InvalidRequest,
            not_found_message,
        ),
        talos_error::TalosError::UnsupportedModel(message) => ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        talos_error::TalosError::ValidationError(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::InternalError,
            other.to_string(),
        ),
    }
}
