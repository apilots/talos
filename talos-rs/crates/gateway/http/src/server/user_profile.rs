use std::sync::Arc;

use axum::extract::State;
use talos_auth::RequestIdentity;
use talos_types::{AuthLogoutRequest, AuthLogoutResponse, UserCurrentRequest, UserCurrentResponse};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn logout_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<AuthLogoutResponse>, ApiError> {
    let actor = shared_actor(&actor.0);
    let response = state
        .gateway
        .logout(&actor, AuthLogoutRequest {})
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn current_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<UserCurrentResponse>, ApiError> {
    let actor = shared_actor(&actor.0);
    let response = state
        .gateway
        .current_user(&actor, UserCurrentRequest {})
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
