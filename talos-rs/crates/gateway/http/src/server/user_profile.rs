use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_auth::{
    AdminCreateUserRequest, AdminPatchUserRequest, LoginRequest, RegisterRequest, RequestIdentity,
    UpdateCurrentUserRequest,
};

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn register_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<Value>, ApiError> {
    let response = state
        .auth_service
        .register(request)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(response)))
}

pub(super) async fn login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<Value>, ApiError> {
    let response = state
        .auth_service
        .login(request)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(response)))
}

pub(super) async fn logout_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    state
        .auth_service
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
        .user_service
        .get_current_user(&actor.0)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(user)))
}

pub(super) async fn patch_current_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<UpdateCurrentUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let user = state
        .user_service
        .patch_current_user(&actor.0, request)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(user)))
}

pub(super) async fn list_admin_users_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let users = state
        .user_service
        .list_users(&actor.0)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!({ "data": users })))
}

pub(super) async fn create_admin_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<AdminCreateUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let user = state
        .user_service
        .create_user(&actor.0, request)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(user)))
}

pub(super) async fn patch_admin_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(user_id): Path<String>,
    Json(request): Json<AdminPatchUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let user = state
        .user_service
        .patch_user(&actor.0, &user_id, request)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(user)))
}

pub(super) async fn delete_admin_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(user_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .user_service
        .delete_user(&actor.0, &user_id)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!({ "success": true })))
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
