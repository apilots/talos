use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;

use talos_auth::RequestIdentity;
use talos_types::{AuthLogoutRequest, AuthLogoutResponse, UserCurrentRequest, UserCurrentResponse};

use super::access::shared_actor;
use super::errors::{ApiError, HttpApiErrorCode};
use super::state::HttpApiState;

pub(super) async fn logout_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<AuthLogoutResponse>, ApiError> {
    let actor = shared_actor(&actor.0);
    let response = state
        .gateway
        .logout(&actor, AuthLogoutRequest {})
        .await
        .map_err(map_auth_error)?;
    Ok(Json(response))
}

pub(super) async fn current_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<UserCurrentResponse>, ApiError> {
    let actor = shared_actor(&actor.0);
    let response = state
        .gateway
        .current_user(&actor, UserCurrentRequest {})
        .await
        .map_err(map_auth_error)?;
    Ok(Json(response))
}

fn map_auth_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::AuthError(message) => ApiError::new(
            StatusCode::UNAUTHORIZED,
            HttpApiErrorCode::Unauthorized,
            message,
        ),
        talos_error::TalosError::ValidationError(message) => ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        talos_error::TalosError::ResourceNotFound(message) => ApiError::new(
            StatusCode::NOT_FOUND,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::InternalError,
            other.to_string(),
        ),
    }
}
