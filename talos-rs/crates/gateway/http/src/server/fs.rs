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
use super::errors::{ApiError, HttpApiErrorCode};
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
            HttpApiErrorCode::PermissionDenied,
            "permission denied",
        ),
        talos_error::TalosError::StorageError(message) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::InternalError,
            message,
        ),
        other => ApiError::from_talos_error(other),
    }
}
