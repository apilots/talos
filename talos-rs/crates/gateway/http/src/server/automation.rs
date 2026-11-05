//! Public automation operations use the original authenticated Gateway.
use super::{
    access::shared_actor,
    errors::{api_success, ApiError, JsonResponse},
    state::HttpApiState,
};
use axum::{
    extract::{Extension, State},
    response::Json,
};
use std::sync::Arc;
use talos_auth::RequestIdentity;
pub(super) async fn session_automation_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<talos_types::SessionAutomationReadRequest>,
) -> Result<JsonResponse<talos_types::SessionAutomationSnapshot>, ApiError> {
    state
        .gateway
        .session_automation(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
pub(super) async fn execute_automation_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<talos_types::AutomationRequest>,
) -> Result<JsonResponse<talos_types::AutomationControlResult>, ApiError> {
    state
        .gateway
        .execute_automation(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
pub(super) async fn list_automation_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<talos_types::AutomationListRequest>,
) -> Result<JsonResponse<talos_types::AutomationListResponse>, ApiError> {
    state
        .gateway
        .list_automation(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
pub(super) async fn automation_command_catalog_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<talos_types::AutomationCommandCatalogRequest>,
) -> Result<JsonResponse<talos_types::AutomationCommandCatalogResponse>, ApiError> {
    state
        .gateway
        .automation_command_catalog(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
pub(super) async fn resolve_automation_command_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<talos_types::AutomationCommandResolveRequest>,
) -> Result<JsonResponse<talos_types::AutomationCommandResolveResponse>, ApiError> {
    state
        .gateway
        .resolve_automation_command(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
pub(super) async fn list_builtin_jobs_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<talos_types::BuiltinJobListRequest>,
) -> Result<JsonResponse<talos_types::BuiltinJobListResponse>, ApiError> {
    state
        .gateway
        .list_builtin_jobs(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
pub(super) async fn control_builtin_job_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Extension<RequestIdentity>,
    Json(request): Json<talos_types::BuiltinJobControlRequest>,
) -> Result<JsonResponse<talos_types::BuiltinJobInfo>, ApiError> {
    state
        .gateway
        .control_builtin_job(&shared_actor(&actor.0), request)
        .await
        .map(api_success)
        .map_err(ApiError::from_talos_error)
}
