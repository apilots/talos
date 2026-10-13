use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::Json;

use talos_auth::RequestIdentity;
use talos_service::WorkspaceDocService;
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, WorkspaceDocListRequest, WorkspaceDocListResponse,
    WorkspaceDocReadRequest, WorkspaceDocReadResponse, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn workspace_info_handler(
    Query(request): Query<talos_types::WorkspaceInfoRequest>,
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<talos_types::WorkspaceInfoResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .workspace_info(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn list_workspace_docs_handler(
    Query(request): Query<WorkspaceDocListRequest>,
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<WorkspaceDocListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .list_workspace_docs(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn get_workspace_doc_handler(
    Query(target): Query<talos_types::WorkspaceInfoRequest>,
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
) -> Result<JsonResponse<WorkspaceDocReadResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let kind = WorkspaceDocService::parse_kind(&kind).map_err(ApiError::from_talos_error)?;
    let detail = state
        .gateway
        .read_workspace_doc(
            &actor,
            WorkspaceDocReadRequest {
                node_id: target.node_id,
                kind,
            },
        )
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(detail))
}

pub(super) async fn put_workspace_doc_handler(
    Query(target): Query<talos_types::WorkspaceInfoRequest>,
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
                node_id: target.node_id,
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
