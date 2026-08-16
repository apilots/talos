use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginPersistence, CloudLoginStartRequest,
    CloudLoginStartResponse, CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_service::Actor;
use talos_types::CloudLoginCancelResponse;

use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn start_cloud_login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<CloudLoginStartRequest>,
) -> Result<JsonResponse<CloudLoginStartResponse>, ApiError> {
    let response = state
        .gateway
        .start_cloud_login(
            &Actor::regular("anonymous"),
            request,
            CloudLoginPersistence::None,
        )
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn cloud_login_status_handler(
    State(state): State<Arc<HttpApiState>>,
    axum::extract::Query(request): axum::extract::Query<CloudLoginStatusRequest>,
) -> Result<JsonResponse<CloudLoginStatusResponse>, ApiError> {
    let response = state
        .gateway
        .cloud_login_status(&Actor::regular("anonymous"), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn cancel_cloud_login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<CloudLoginCancelRequest>,
) -> Result<JsonResponse<CloudLoginCancelResponse>, ApiError> {
    let response = state
        .gateway
        .cancel_cloud_login(&Actor::regular("anonymous"), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
