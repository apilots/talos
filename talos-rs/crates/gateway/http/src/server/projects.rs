use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_types::ProjectPatch;

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

#[derive(Debug, Deserialize)]
pub(super) struct CreateProjectRequest {
    name: String,
    directory: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PatchProjectRequest {
    #[serde(default)]
    name: Option<String>,
}

pub(super) async fn list_projects_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let projects = state
        .app
        .list_projects(&shared_actor(&actor.0))
        .await
        .map_err(map_project_error)?;
    Ok(Json(json!({ "data": projects })))
}

pub(super) async fn get_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let project = state
        .app
        .get_project(&shared_actor(&actor.0), &project_id)
        .await
        .map_err(map_project_error)?;
    Ok(Json(json!(project)))
}

pub(super) async fn create_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<CreateProjectRequest>,
) -> Result<Json<Value>, ApiError> {
    let project = state
        .app
        .create_project(&shared_actor(&actor.0), request.name, request.directory)
        .await
        .map_err(map_project_error)?;
    Ok(Json(json!(project)))
}

pub(super) async fn patch_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
    Json(request): Json<PatchProjectRequest>,
) -> Result<Json<Value>, ApiError> {
    let project = state
        .app
        .patch_project(
            &shared_actor(&actor.0),
            &project_id,
            ProjectPatch { name: request.name },
        )
        .await
        .map_err(map_project_error)?;

    Ok(Json(json!(project)))
}

pub(super) async fn delete_project_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .app
        .delete_project(&shared_actor(&actor.0), &project_id)
        .await
        .map_err(map_project_delete_error)?;
    Ok(Json(json!({ "success": true })))
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
