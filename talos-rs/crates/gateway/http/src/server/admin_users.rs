pub(super) use super::user_profile::{
    create_admin_user_handler, delete_admin_user_handler, list_admin_users_handler,
    patch_admin_user_handler,
};

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_config::LlmConfig;
use talos_types::SessionQuery;

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn list_admin_sessions_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Value>, ApiError> {
    let sessions = state
        .access_service
        .list_admin_sessions(&actor.0, query)
        .await
        .map_err(map_admin_error)?;
    Ok(Json(json!({ "data": sessions })))
}

pub(super) async fn list_admin_projects_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let projects = state
        .access_service
        .list_admin_projects(&actor.0)
        .await
        .map_err(map_admin_error)?;
    Ok(Json(json!({ "data": projects })))
}

pub(super) async fn get_admin_llm_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<LlmConfig>, ApiError> {
    require_admin(&actor.0)?;
    let llm = state
        .llm_config_service
        .get()
        .map_err(map_admin_error)?;
    Ok(Json(llm))
}

pub(super) async fn put_admin_llm_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(llm): Json<LlmConfig>,
) -> Result<Json<LlmConfig>, ApiError> {
    require_admin(&actor.0)?;
    let new_llm = state
        .llm_config_service
        .update(llm)
        .map_err(map_admin_error)?;
    state.reload_llm(&new_llm).map_err(map_admin_error)?;
    let result = state
        .llm_config_service
        .get()
        .map_err(map_admin_error)?;
    Ok(Json(result))
}

fn require_admin(actor: &RequestIdentity) -> Result<(), ApiError> {
    if actor.role.is_admin() {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        "forbidden",
        "admin access required",
    ))
}

fn map_admin_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::AuthError(message) => {
            ApiError::new(StatusCode::FORBIDDEN, "forbidden", message)
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
