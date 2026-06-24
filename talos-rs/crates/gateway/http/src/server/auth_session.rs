use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginPersistence, CloudLoginStartRequest,
    CloudLoginStartResponse, CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_service::Actor;
use talos_types::CloudLoginCancelResponse;

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn start_cloud_login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<CloudLoginStartRequest>,
) -> Result<Json<CloudLoginStartResponse>, ApiError> {
    Ok(Json(
        state
            .gateway
            .start_cloud_login(
                &Actor::regular("anonymous"),
                request,
                CloudLoginPersistence::None,
            )
            .await
            .map_err(ApiError::from_talos_error)?,
    ))
}

pub(super) async fn cloud_login_status_handler(
    State(state): State<Arc<HttpApiState>>,
    axum::extract::Query(request): axum::extract::Query<CloudLoginStatusRequest>,
) -> Result<Json<CloudLoginStatusResponse>, ApiError> {
    Ok(Json(
        state
            .gateway
            .cloud_login_status(&Actor::regular("anonymous"), request)
            .await
            .map_err(ApiError::from_talos_error)?,
    ))
}

pub(super) async fn cancel_cloud_login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<CloudLoginCancelRequest>,
) -> Result<Json<CloudLoginCancelResponse>, ApiError> {
    let response = state
        .gateway
        .cancel_cloud_login(&Actor::regular("anonymous"), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(response))
}
