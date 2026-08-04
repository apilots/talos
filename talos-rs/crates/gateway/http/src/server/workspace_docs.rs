use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::Json;

use talos_auth::RequestIdentity;
use talos_service::WorkspaceDocService;
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, WorkspaceDocListRequest, WorkspaceDocListResponse,
    WorkspaceDocReadRequest, WorkspaceDocReadResponse, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse, WorkspaceInfoRequest, WorkspaceInfoResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn list_workspace_docs_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<WorkspaceDocListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .list_workspace_docs(&actor, WorkspaceDocListRequest {})
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn get_workspace_info_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<WorkspaceInfoResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let info = state
        .gateway
        .workspace_info(&actor, WorkspaceInfoRequest {})
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(info))
}

pub(super) async fn get_workspace_doc_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
) -> Result<JsonResponse<WorkspaceDocReadResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let kind = WorkspaceDocService::parse_kind(&kind).map_err(ApiError::from_talos_error)?;
    let detail = state
        .gateway
        .read_workspace_doc(&actor, WorkspaceDocReadRequest { kind })
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(detail))
}

pub(super) async fn put_workspace_doc_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
    Json(request): Json<DocUpdateRequest>,
) -> Result<JsonResponse<WorkspaceDocWriteResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let kind = WorkspaceDocService::parse_kind(&kind).map_err(ApiError::from_talos_error)?;
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
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(detail))
}

fn expected_file_version_token(value: ExpectedFileVersion) -> Option<String> {
    match value {
        ExpectedFileVersion::MustNotExist => None,
        ExpectedFileVersion::Match { version_token } => Some(version_token),
    }
}
