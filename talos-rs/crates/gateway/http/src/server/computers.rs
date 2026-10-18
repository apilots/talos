use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::sync::Arc;
use talos_auth::RequestIdentity;

pub(super) fn routes() -> axum::Router<Arc<HttpApiState>> {
    use axum::routing::{get, patch, post, put};
    axum::Router::new()
        .route(
            "/v1/computers",
            get(list_computers_handler).post(create_computer_handler),
        )
        .route(
            "/v1/computers/:node_id",
            get(read_computer_handler).delete(delete_computer_handler),
        )
        .route(
            "/v1/computers/:node_id/name",
            patch(rename_computer_handler),
        )
        .route(
            "/v1/computers/:node_id/status",
            put(set_computer_status_handler),
        )
        .route(
            "/v1/computers/:node_id/credential",
            post(rotate_computer_credential_handler),
        )
}

pub(super) async fn list_computers_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Query(request): Query<talos_types::ComputerListRequest>,
) -> Result<JsonResponse<talos_types::ComputerListResponse>, ApiError> {
    let response = state
        .gateway
        .list_computers(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn read_computer_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(node_id): Path<String>,
) -> Result<JsonResponse<talos_types::NodeInfo>, ApiError> {
    let request = talos_types::GetNodeRequest { node_id };
    let response = state
        .gateway
        .read_computer(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn create_computer_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<talos_types::CreateNodeRequest>,
) -> Result<Response, ApiError> {
    let response = state
        .gateway
        .create_computer(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        api_success(response),
    )
        .into_response())
}

pub(super) async fn rename_computer_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(node_id): Path<String>,
    Json(mut request): Json<talos_types::RenameComputerRequest>,
) -> Result<JsonResponse<talos_types::NodeInfo>, ApiError> {
    request.node_id = node_id;
    let response = state
        .gateway
        .rename_computer(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn set_computer_status_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(node_id): Path<String>,
    Json(mut request): Json<talos_types::SetComputerStatusRequest>,
) -> Result<JsonResponse<talos_types::NodeInfo>, ApiError> {
    request.node_id = node_id;
    let response = state
        .gateway
        .set_computer_status(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn rotate_computer_credential_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(node_id): Path<String>,
    Json(mut request): Json<talos_types::RotateCredentialRequest>,
) -> Result<Response, ApiError> {
    request.node_id = node_id;
    let response = state
        .gateway
        .rotate_computer_credential(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        api_success(response),
    )
        .into_response())
}

pub(super) async fn delete_computer_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(node_id): Path<String>,
) -> Result<JsonResponse<talos_types::DeleteComputerResponse>, ApiError> {
    let response = state
        .gateway
        .delete_computer(
            &shared_actor(&actor.0),
            talos_types::DeleteComputerRequest { node_id },
        )
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
