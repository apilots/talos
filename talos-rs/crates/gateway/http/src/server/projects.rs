use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::Json;

use talos_auth::RequestIdentity;
use talos_types::{
    ProjectCreateRequest, ProjectCreateResponse, ProjectDeleteRequest, ProjectDeleteResponse,
    ProjectListRequest, ProjectListResponse, ProjectPatchRequest, ProjectPatchResponse,
    ProjectReadRequest, ProjectReadResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn list_projects_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<ProjectListResponse>, ApiError> {
    let response = state
        .gateway
        .list_projects(&shared_actor(&actor.0), ProjectListRequest::default())
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn get_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
) -> Result<JsonResponse<ProjectReadResponse>, ApiError> {
    let response = state
        .gateway
        .get_project(&shared_actor(&actor.0), ProjectReadRequest { project_id })
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn create_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<ProjectCreateRequest>,
) -> Result<JsonResponse<ProjectCreateResponse>, ApiError> {
    let response = state
        .gateway
        .create_project(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn patch_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
    Json(mut request): Json<ProjectPatchRequest>,
) -> Result<JsonResponse<ProjectPatchResponse>, ApiError> {
    request.project_id = project_id;
    let response = state
        .gateway
        .patch_project(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;

    Ok(api_success(response))
}

pub(super) async fn delete_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
) -> Result<JsonResponse<ProjectDeleteResponse>, ApiError> {
    let response = state
        .gateway
        .delete_project(&shared_actor(&actor.0), ProjectDeleteRequest { project_id })
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
