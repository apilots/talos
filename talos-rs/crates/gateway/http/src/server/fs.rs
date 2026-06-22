use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;

use talos_auth::RequestIdentity;
use talos_types::{
    FsDirectoryCreateRequest, FsDirectoryCreateResponse, FsDirectoryReadRequest,
    FsDirectoryReadResponse,
};

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn read_directory_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Query(request): Query<FsDirectoryReadRequest>,
) -> Result<Json<FsDirectoryReadResponse>, ApiError> {
    let response = state
        .gateway
        .read_fs_directory(&shared_actor(&actor.0), request)
        .await
        .map_err(map_fs_error)?;
    Ok(Json(response))
}

pub(super) async fn create_directory_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<FsDirectoryCreateRequest>,
) -> Result<Json<FsDirectoryCreateResponse>, ApiError> {
    let response = state
        .gateway
        .create_fs_directory(&shared_actor(&actor.0), request)
        .await
        .map_err(map_fs_error)?;
    Ok(Json(response))
}

fn map_fs_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::AuthorizationDenied => ApiError::new(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "permission denied",
        ),
        talos_error::TalosError::ResourceNotFound(message) => ApiError::new(
            StatusCode::NOT_FOUND,
            fs_code(&message, "path_not_found"),
            message,
        ),
        talos_error::TalosError::Conflict(message) => ApiError::new(
            StatusCode::CONFLICT,
            fs_code(&message, "already_exists"),
            message,
        ),
        talos_error::TalosError::ValidationError(message) => {
            let code = fs_code(&message, "invalid_path");
            let status = match code {
                "fs_disabled" | "path_outside_root" => StatusCode::FORBIDDEN,
                _ => StatusCode::BAD_REQUEST,
            };
            ApiError::new(status, code, message)
        }
        talos_error::TalosError::StorageError(message) => {
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
        }
        other => ApiError::from_talos_error(other),
    }
}

fn fs_code(message: &str, fallback: &'static str) -> &'static str {
    if message.starts_with("invalid_path:") {
        "invalid_path"
    } else if message.starts_with("fs_disabled:") {
        "fs_disabled"
    } else if message.starts_with("path_outside_root:") {
        "path_outside_root"
    } else if message.starts_with("symlink_not_supported:") {
        "symlink_not_supported"
    } else if message.starts_with("path_not_found:") {
        "path_not_found"
    } else if message.starts_with("not_directory:") {
        "not_directory"
    } else if message.starts_with("invalid_name:") {
        "invalid_name"
    } else if message.starts_with("already_exists:") {
        "already_exists"
    } else {
        fallback
    }
}
