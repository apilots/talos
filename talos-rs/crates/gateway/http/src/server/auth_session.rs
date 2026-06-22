use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use talos_application_service::Actor;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_types::{
    AuthSessionClearRequest, AuthSessionClearResponse, AuthSessionReadRequest,
    AuthSessionReadResponse, AuthSessionStoreRequest, AuthSessionStoreResponse,
    CloudLoginCancelResponse,
};

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;
use talos_auth::RequestIdentity;

pub(super) async fn read_auth_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<AuthSessionReadResponse>, ApiError> {
    let actor = shared_actor(&actor.0);
    let response = state
        .gateway
        .read_auth_session(&actor, AuthSessionReadRequest {})
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(response))
}

pub(super) async fn start_cloud_login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<CloudLoginStartRequest>,
) -> Result<Json<CloudLoginStartResponse>, ApiError> {
    Ok(Json(
        state
            .gateway
            .start_cloud_login(&Actor::regular("anonymous"), request)
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

pub(super) async fn store_auth_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(params): Json<AuthSessionStoreRequest>,
) -> Result<Json<AuthSessionStoreResponse>, ApiError> {
    let actor = shared_actor(&actor.0);
    let response = state
        .gateway
        .store_auth_session(&actor, params)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(response))
}

pub(super) async fn clear_auth_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<AuthSessionClearResponse>, ApiError> {
    let actor = shared_actor(&actor.0);
    let response = state
        .gateway
        .clear_auth_session(&actor, AuthSessionClearRequest {})
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(response))
}
