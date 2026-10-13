use super::{
    access::shared_actor,
    errors::{api_success, ApiError, JsonResponse},
    state::HttpApiState,
};
use axum::{
    extract::State,
    routing::{get, post},
    Extension, Json, Router,
};
use std::sync::Arc;
use talos_auth::RequestIdentity;
use talos_types::{SyncEnableRequest, SyncStatusResponse};

pub(super) fn routes() -> Router<Arc<HttpApiState>> {
    Router::new()
        .route("/v1/sync/status", get(status))
        .route("/v1/sync/enable", post(enable))
        .route("/v1/sync/disable", post(disable))
}
async fn status(
    State(state): State<Arc<HttpApiState>>,
    Extension(actor): Extension<RequestIdentity>,
) -> Result<JsonResponse<SyncStatusResponse>, ApiError> {
    state
        .app
        .sync_status(&shared_actor(&actor))
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
async fn enable(
    State(state): State<Arc<HttpApiState>>,
    Extension(actor): Extension<RequestIdentity>,
    Json(request): Json<SyncEnableRequest>,
) -> Result<JsonResponse<SyncStatusResponse>, ApiError> {
    state
        .app
        .enable_sync(&shared_actor(&actor), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
async fn disable(
    State(state): State<Arc<HttpApiState>>,
    Extension(actor): Extension<RequestIdentity>,
) -> Result<JsonResponse<SyncStatusResponse>, ApiError> {
    state
        .app
        .disable_sync(&shared_actor(&actor))
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
