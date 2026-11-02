//! Typed HTTP projection of the shared Session trajectory Service operations.
use super::{
    access::shared_actor,
    errors::{api_success, ApiError, JsonResponse},
    state::HttpApiState,
};
use axum::extract::{Extension, State};
use axum::Json;
use std::sync::Arc;
use talos_agent_api::trajectory::*;
use talos_auth::RequestIdentity;
pub(super) async fn trajectory_capabilities(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
) -> Result<JsonResponse<TrajectoryCapabilitiesResponse>, ApiError> {
    let response = state
        .gateway
        .trajectory_capabilities(&shared_actor(&actor.0))
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
pub(super) async fn read_turn_trajectory(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<TrajectoryReadTurnRequest>,
) -> Result<JsonResponse<TrajectoryReadTurnResponse>, ApiError> {
    let response = state
        .gateway
        .read_turn_trajectory(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
pub(super) async fn read_request_trajectory(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<TrajectoryReadModelRequestRequest>,
) -> Result<JsonResponse<TrajectoryReadModelRequestResponse>, ApiError> {
    let response = state
        .gateway
        .read_request_trajectory(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
pub(super) async fn read_tool_trajectory(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<TrajectoryReadToolRequest>,
) -> Result<JsonResponse<TrajectoryReadToolResponse>, ApiError> {
    let response = state
        .gateway
        .read_tool_trajectory(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
pub(super) async fn read_trajectory_payload(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<TrajectoryReadPayloadRequest>,
) -> Result<JsonResponse<TrajectoryReadPayloadResponse>, ApiError> {
    let response = state
        .gateway
        .read_trajectory_payload(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
