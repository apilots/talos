use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use talos_auth::AuthResult;
use talos_types::talos::TalosContext;
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, SessionConf, SessionPatch, SessionQuery,
};

use super::errors::ApiError;
use super::misc::{validate_auth_context, validate_model_supported};
use super::state::HttpApiState;

#[derive(Debug, Deserialize)]
pub(super) struct CreateSessionRequest {
    context: TalosContext,
    model: Option<String>,
    #[serde(default)]
    conf: Option<SessionConf>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    current_directory: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PatchSessionRequest {
    #[serde(default)]
    conf: Option<SessionConf>,
    #[serde(default)]
    project_id: Option<OptionalProjectIdPatch>,
    #[serde(default)]
    current_directory: Option<OptionalCurrentDirectoryPatch>,
}

pub(super) async fn create_session_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<AuthResult>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_auth_context(
        &auth.0,
        &request.context,
        super::DEFAULT_SCOPE_SESSIONS_WRITE,
    )?;
    if request.context.project_root.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "context.project_root is not supported",
        ));
    }
    if let Some(model) = request.model.as_deref() {
        validate_model_supported(&state, &model)?;
    }

    let session = state
        .session_service
        .create_session(
            request.context,
            request.model,
            request.conf,
            request.project_id,
            request.current_directory,
        )
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;

    Ok(Json(json!(session)))
}

pub(super) async fn list_sessions_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Value>, ApiError> {
    let sessions = state
        .session_service
        .list_sessions(query)
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
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let Some(session) = state
        .session_service
        .get_session(&session_id)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?
    else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "session not found",
        ));
    };

    Ok(Json(json!(session)))
}

pub(super) async fn get_session_turns_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let turns = state
        .session_service
        .get_session_turns(&session_id)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;
    Ok(Json(json!({ "data": turns })))
}

pub(super) async fn patch_session_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
    Json(request): Json<PatchSessionRequest>,
) -> Result<Json<Value>, ApiError> {
    let patch = SessionPatch {
        conf: request.conf,
        project_id: request.project_id,
        current_directory: request.current_directory,
    };
    if let Some(model) = patch
        .agent_model()
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()))?
    {
        validate_model_supported(&state, &model)?;
    }
    let ack = state
        .session_service
        .patch_session(&session_id, patch)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;

    if !ack.success {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "session not found",
        ));
    }

    Ok(Json(json!({ "success": true })))
}

pub(super) async fn close_session_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .session_service
        .close_session(&session_id)
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
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let ack = state
        .session_service
        .delete_session(&session_id)
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
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .session_service
        .compact_session(&session_id)
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
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .session_service
        .clear_session(&session_id)
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
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .session_service
        .cancel_active_turn(&session_id)
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
