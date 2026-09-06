use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use talos_auth::RequestIdentity;
use talos_types::{
    MessageQueue, MessageQueueAddRequest, MessageQueueDeleteRequest, MessageQueueListRequest,
    MessageQueueReorderRequest, MessageQueueStartRequest, MessageQueueUpdateRequest,
    StartTurnResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn add_queued_message_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<MessageQueueAddRequest>,
) -> Result<JsonResponse<MessageQueue>, ApiError> {
    state
        .gateway
        .add_queued_message(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}

pub(super) async fn list_message_queue_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<MessageQueueListRequest>,
) -> Result<JsonResponse<MessageQueue>, ApiError> {
    state
        .gateway
        .list_message_queue(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}

pub(super) async fn update_queued_message_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<MessageQueueUpdateRequest>,
) -> Result<JsonResponse<MessageQueue>, ApiError> {
    state
        .gateway
        .update_queued_message(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}

pub(super) async fn delete_queued_message_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<MessageQueueDeleteRequest>,
) -> Result<JsonResponse<MessageQueue>, ApiError> {
    state
        .gateway
        .delete_queued_message(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}

pub(super) async fn reorder_message_queue_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<MessageQueueReorderRequest>,
) -> Result<JsonResponse<MessageQueue>, ApiError> {
    state
        .gateway
        .reorder_message_queue(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}

pub(super) async fn start_queued_message_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<MessageQueueStartRequest>,
) -> Result<JsonResponse<StartTurnResponse>, ApiError> {
    state
        .gateway
        .start_queued_message(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
