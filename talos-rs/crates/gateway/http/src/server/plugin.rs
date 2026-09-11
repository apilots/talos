use std::sync::Arc;

use axum::extract::{Json, Query, State};
use talos_auth::RequestIdentity;
use talos_types::{
    PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstallRequest, PluginInstallResponse, PluginListRequest, PluginListResponse,
    PluginReadRequest, PluginReadResponse, PluginSearchRequest, PluginSearchResponse,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn list_plugin_sources_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Query(request): Query<PluginSourceListRequest>,
) -> Result<JsonResponse<PluginSourceListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .list_plugin_sources(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn add_plugin_source_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginSourceAddRequest>,
) -> Result<JsonResponse<PluginSourceAddResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .add_plugin_source(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn refresh_plugin_source_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginSourceRefreshRequest>,
) -> Result<JsonResponse<PluginSourceRefreshResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .refresh_plugin_source(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn remove_plugin_source_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginSourceRemoveRequest>,
) -> Result<JsonResponse<PluginSourceRemoveResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .remove_plugin_source(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn search_plugins_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginSearchRequest>,
) -> Result<JsonResponse<PluginSearchResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .search_plugins(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn list_plugins_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginListRequest>,
) -> Result<JsonResponse<PluginListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .list_plugins(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn read_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginReadRequest>,
) -> Result<JsonResponse<PluginReadResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .read_plugin(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn install_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginInstallRequest>,
) -> Result<JsonResponse<PluginInstallResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .install_plugin(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn update_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginUpdateRequest>,
) -> Result<JsonResponse<PluginUpdateResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .update_plugin(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn uninstall_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginUninstallRequest>,
) -> Result<JsonResponse<PluginUninstallResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .uninstall_plugin(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn enable_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginEnableRequest>,
) -> Result<JsonResponse<PluginEnableResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .enable_plugin(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn disable_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<PluginDisableRequest>,
) -> Result<JsonResponse<PluginDisableResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .disable_plugin(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn list_mcp_servers_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Query(request): Query<talos_types::McpServerListRequest>,
) -> Result<JsonResponse<talos_types::McpServerListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .list_mcp_servers(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn import_mcp_server_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<talos_types::McpServerImportRequest>,
) -> Result<JsonResponse<talos_types::PluginInstallResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .import_mcp_server(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn start_mcp_authentication_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<talos_types::McpAuthenticationStartRequest>,
) -> Result<JsonResponse<talos_types::McpAuthenticationStartResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .start_mcp_authentication(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn read_mcp_authentication_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<talos_types::McpAuthenticationReadRequest>,
) -> Result<JsonResponse<talos_types::McpAuthenticationResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .read_mcp_authentication(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn update_mcp_credentials_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<talos_types::McpCredentialsUpdateRequest>,
) -> Result<JsonResponse<talos_types::McpAuthenticationResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .update_mcp_credentials(&actor, request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
