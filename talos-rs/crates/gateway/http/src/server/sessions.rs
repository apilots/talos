use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_types::talos::SessionHistorySnapshot;
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, SessionConf, SessionPatch, SessionQuery,
};

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

#[derive(Debug, Deserialize)]
pub(super) struct PatchSessionRequest {
    #[serde(default)]
    conf: Option<SessionConf>,
    #[serde(default)]
    project_id: Option<OptionalProjectIdPatch>,
    #[serde(default)]
    current_directory: Option<OptionalCurrentDirectoryPatch>,
}

pub(super) async fn list_sessions_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Value>, ApiError> {
    let sessions = state
        .app
        .list_sessions(&shared_actor(&actor.0), query)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;
    let data: Vec<Value> = sessions.into_iter().map(|s| json!(s)).collect();

    Ok(Json(json!({ "data": data })))
}

pub(super) async fn get_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .app
        .get_session(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|e| map_session_error(e, "session not found"))?;

    Ok(Json(json!(session)))
}

pub(super) async fn get_session_history_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionHistorySnapshot>, ApiError> {
    let snapshot = state
        .app
        .get_session_history(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|e| map_session_error(e, "session not found"))?;

    Ok(Json(snapshot))
}

pub(super) async fn patch_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    Json(request): Json<PatchSessionRequest>,
) -> Result<Json<Value>, ApiError> {
    let patch = SessionPatch {
        conf: request.conf,
        project_id: request.project_id,
        current_directory: request.current_directory,
    };
    let _ack = state
        .app
        .patch_session(&shared_actor(&actor.0), &session_id, patch)
        .await
        .map_err(|e| map_patch_session_error(e, "session not found"))?;

    Ok(Json(json!({ "success": true })))
}

pub(super) async fn close_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .app
        .close_session(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                "invalid_request",
                "session not found",
            ),
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                other.to_string(),
            ),
        })?;
    Ok(Json(json!({ "success": true })))
}

pub(super) async fn delete_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let ack = state
        .app
        .delete_session(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                "invalid_request",
                "session not found",
            ),
            talos_error::TalosError::ValidationError(message) => {
                ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
            }
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                other.to_string(),
            ),
        })?;
    Ok(Json(json!({ "success": ack.success })))
}

pub(super) async fn compact_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .app
        .compact_session(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                "invalid_request",
                "session not found",
            ),
            talos_error::TalosError::ValidationError(message) => {
                ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
            }
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
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
        .app
        .clear_session(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                "invalid_request",
                "session not found",
            ),
            talos_error::TalosError::ValidationError(message) => {
                ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
            }
            other => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                other.to_string(),
            ),
        })?;
    Ok(Json(json!(session)))
}

pub(super) async fn cancel_active_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .app
        .cancel_active_turn(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|err| match err {
            talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
                StatusCode::NOT_FOUND,
                "invalid_request",
                "session not found",
            ),
            _ => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                err.to_string(),
            ),
        })?;

    Ok(Json(json!({ "success": true })))
}

fn map_session_error(error: talos_error::TalosError, not_found_message: &str) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => {
            ApiError::new(StatusCode::NOT_FOUND, "invalid_request", not_found_message)
        }
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}

fn map_patch_session_error(error: talos_error::TalosError, not_found_message: &str) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => {
            ApiError::new(StatusCode::NOT_FOUND, "invalid_request", not_found_message)
        }
        talos_error::TalosError::UnsupportedModel(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}
