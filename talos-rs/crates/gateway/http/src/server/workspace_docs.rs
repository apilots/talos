use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_application_service::WorkspaceDocService;
use talos_auth::RequestIdentity;
use talos_error::TalosError;
use talos_types::DocUpdateRequest;

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn list_workspace_docs_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let docs = state
        .workspace_docs
        .list_docs()
        .await
        .map_err(map_workspace_doc_error)?;
    Ok(Json(json!({ "data": docs })))
}

pub(super) async fn get_workspace_doc_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let kind = WorkspaceDocService::parse_kind(&kind).map_err(map_workspace_doc_error)?;
    let detail = state
        .workspace_docs
        .get_doc(kind)
        .await
        .map_err(map_workspace_doc_error)?;
    Ok(Json(json!(detail)))
}

pub(super) async fn put_workspace_doc_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
    Json(request): Json<DocUpdateRequest>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let kind = WorkspaceDocService::parse_kind(&kind).map_err(map_workspace_doc_error)?;
    let detail = state
        .workspace_docs
        .put_doc(&actor, kind, request)
        .await
        .map_err(map_workspace_doc_error)?;
    Ok(Json(json!(detail)))
}

fn map_workspace_doc_error(error: TalosError) -> ApiError {
    match error {
        TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        TalosError::Conflict(message) | TalosError::ResourceNotFound(message) => {
            ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
        }
        TalosError::AuthError(message) => {
            ApiError::new(StatusCode::FORBIDDEN, "forbidden", message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}
