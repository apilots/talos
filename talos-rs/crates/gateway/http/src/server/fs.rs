use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Json;

use talos_auth::RequestIdentity;
use talos_types::{
    FsDirectoryCreateRequest, FsDirectoryCreateResponse, FsDirectoryReadRequest,
    FsDirectoryReadResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn read_directory_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Query(request): Query<FsDirectoryReadRequest>,
) -> Result<JsonResponse<FsDirectoryReadResponse>, ApiError> {
    let response = state
        .gateway
        .read_fs_directory(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn create_directory_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<FsDirectoryCreateRequest>,
) -> Result<JsonResponse<FsDirectoryCreateResponse>, ApiError> {
    let response = state
        .gateway
        .create_fs_directory(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
