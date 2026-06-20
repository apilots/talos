use std::sync::Arc;

use axum::extract::{Json, Query, State};
use axum::response::Json as JsonResponse;
use talos_types::{
    PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstallRequest, PluginInstallResponse, PluginListRequest, PluginListResponse,
    PluginReadRequest, PluginReadResponse, PluginSearchRequest, PluginSearchResponse,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
};

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn list_plugin_sources_handler(
    State(state): State<Arc<HttpApiState>>,
    Query(request): Query<PluginSourceListRequest>,
) -> Result<JsonResponse<PluginSourceListResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .list_sources(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn add_plugin_source_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginSourceAddRequest>,
) -> Result<JsonResponse<PluginSourceAddResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .add_source(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn refresh_plugin_source_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginSourceRefreshRequest>,
) -> Result<JsonResponse<PluginSourceRefreshResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .refresh_source(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn remove_plugin_source_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginSourceRemoveRequest>,
) -> Result<JsonResponse<PluginSourceRemoveResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .remove_source(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn search_plugins_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginSearchRequest>,
) -> Result<JsonResponse<PluginSearchResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .search(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn list_plugins_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginListRequest>,
) -> Result<JsonResponse<PluginListResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .list_plugins(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn read_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginReadRequest>,
) -> Result<JsonResponse<PluginReadResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .read(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn install_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginInstallRequest>,
) -> Result<JsonResponse<PluginInstallResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .install(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn update_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginUpdateRequest>,
) -> Result<JsonResponse<PluginUpdateResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .update(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn uninstall_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginUninstallRequest>,
) -> Result<JsonResponse<PluginUninstallResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .uninstall(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn enable_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginEnableRequest>,
) -> Result<JsonResponse<PluginEnableResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .enable(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}

pub(super) async fn disable_plugin_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<PluginDisableRequest>,
) -> Result<JsonResponse<PluginDisableResponse>, ApiError> {
    let response = state
        .app
        .plugins
        .disable(request)
        .map_err(ApiError::from_talos_error)?;
    Ok(JsonResponse(response))
}
