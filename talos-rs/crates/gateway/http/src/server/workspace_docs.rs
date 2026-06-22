use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_application_service::WorkspaceDocService;
use talos_auth::RequestIdentity;
use talos_error::TalosError;
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, WorkspaceDocListRequest, WorkspaceDocReadRequest,
    WorkspaceDocWriteRequest, WorkspaceInfoRequest,
};

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn list_workspace_docs_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .list_workspace_docs(&actor, WorkspaceDocListRequest {})
        .await
        .map_err(map_workspace_doc_error)?;
    Ok(Json(json!(response)))
}

pub(super) async fn get_workspace_info_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let info = state
        .gateway
        .workspace_info(&actor, WorkspaceInfoRequest {})
        .await
        .map_err(map_workspace_doc_error)?;
    Ok(Json(json!(info)))
}

pub(super) async fn get_workspace_doc_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let kind = WorkspaceDocService::parse_kind(&kind).map_err(map_workspace_doc_error)?;
    let detail = state
        .gateway
        .read_workspace_doc(&actor, WorkspaceDocReadRequest { kind })
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
        .gateway
        .write_workspace_doc(
            &actor,
            WorkspaceDocWriteRequest {
                kind,
                content: request.content,
                expected_version: expected_file_version_token(request.base_version),
            },
        )
        .await
        .map_err(map_workspace_doc_error)?;
    Ok(Json(json!(detail)))
}

fn expected_file_version_token(value: ExpectedFileVersion) -> Option<String> {
    match value {
        ExpectedFileVersion::MustNotExist => None,
        ExpectedFileVersion::Match { version_token } => Some(version_token),
    }
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
