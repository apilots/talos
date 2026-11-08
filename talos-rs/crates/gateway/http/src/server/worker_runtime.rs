use std::sync::Arc;

use axum::{
    extract::State,
    http::header::CACHE_CONTROL,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use talos_auth::RequestIdentity;
use talos_types::{WorkerRuntimeCandidate, WorkerRuntimeSettingsResponse};

use super::{
    access::shared_actor,
    errors::{api_success, ApiError, JsonResponse},
    state::HttpApiState,
};

pub(super) fn routes() -> Router<Arc<HttpApiState>> {
    Router::new()
        .route("/v1/worker-runtime", get(read).put(update))
        .route("/v1/worker-runtime/validate", post(validate))
}

async fn read(
    State(state): State<Arc<HttpApiState>>,
    Extension(actor): Extension<RequestIdentity>,
) -> Response {
    let result = state
        .app
        .read_worker_runtime_settings(&shared_actor(&actor), &state.base_config)
        .map(api_success)
        .map_err(ApiError::from_talos_error);
    ([(CACHE_CONTROL, "no-store")], result).into_response()
}

async fn validate(
    State(state): State<Arc<HttpApiState>>,
    Extension(actor): Extension<RequestIdentity>,
    Json(request): Json<WorkerRuntimeCandidate>,
) -> Result<JsonResponse<()>, ApiError> {
    state
        .app
        .validate_worker_runtime_settings(&shared_actor(&actor), &state.base_config, request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}

async fn update(
    State(state): State<Arc<HttpApiState>>,
    Extension(actor): Extension<RequestIdentity>,
    Json(request): Json<WorkerRuntimeCandidate>,
) -> Result<JsonResponse<WorkerRuntimeSettingsResponse>, ApiError> {
    state
        .app
        .update_worker_runtime_settings(&shared_actor(&actor), &state.base_config, request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
