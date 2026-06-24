use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;

use talos_auth::RequestIdentity;
use talos_types::{
    ProjectCreateRequest, ProjectCreateResponse, ProjectDeleteRequest, ProjectDeleteResponse,
    ProjectListRequest, ProjectListResponse, ProjectPatchRequest, ProjectPatchResponse,
    ProjectReadRequest, ProjectReadResponse,
};

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn list_projects_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<ProjectListResponse>, ApiError> {
    let response = state
        .gateway
        .list_projects(&shared_actor(&actor.0), ProjectListRequest::default())
        .await
        .map_err(map_project_error)?;
    Ok(Json(response))
}

pub(super) async fn get_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
) -> Result<Json<ProjectReadResponse>, ApiError> {
    let response = state
        .gateway
        .get_project(&shared_actor(&actor.0), ProjectReadRequest { project_id })
        .await
        .map_err(map_project_error)?;
    Ok(Json(response))
}

pub(super) async fn create_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<ProjectCreateRequest>,
) -> Result<Json<ProjectCreateResponse>, ApiError> {
    let response = state
        .gateway
        .create_project(&shared_actor(&actor.0), request)
        .await
        .map_err(map_project_error)?;
    Ok(Json(response))
}

pub(super) async fn patch_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
    Json(mut request): Json<ProjectPatchRequest>,
) -> Result<Json<ProjectPatchResponse>, ApiError> {
    request.project_id = project_id;
    let response = state
        .gateway
        .patch_project(&shared_actor(&actor.0), request)
        .await
        .map_err(map_project_error)?;

    Ok(Json(response))
}

pub(super) async fn delete_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
) -> Result<Json<ProjectDeleteResponse>, ApiError> {
    let response = state
        .gateway
        .delete_project(&shared_actor(&actor.0), ProjectDeleteRequest { project_id })
        .await
        .map_err(map_project_delete_error)?;
    Ok(Json(response))
}

fn map_project_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(message) => {
            ApiError::new(StatusCode::NOT_FOUND, "invalid_request", message)
        }
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}

fn map_project_delete_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
        }
        other => map_project_error(other),
    }
}
