use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn local_auth_removed_handler() -> Result<Json<Value>, ApiError> {
    Err(ApiError::new(
        StatusCode::GONE,
        "local_auth_removed",
        "Talos local login and registration have moved to Talos Cloud",
    ))
}

pub(super) async fn logout_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    state
        .account
        .logout(&actor.0)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!({ "success": true })))
}

pub(super) async fn current_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let user = state
        .account
        .current_user(&actor.0)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(user)))
}

fn map_auth_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::AuthError(message) => {
            ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
        }
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        talos_error::TalosError::ResourceNotFound(message) => {
            ApiError::new(StatusCode::NOT_FOUND, "invalid_request", message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}
