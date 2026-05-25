use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_auth::{
    AdminCreateUserRequest, AdminPatchUserRequest, CreateUserTokenRequest, RequestIdentity,
    UpdateCurrentUserRequest,
};

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

pub(super) async fn patch_current_user_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<UpdateCurrentUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let user = state
        .account
        .patch_current_user(&actor.0, request)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(user)))
}

pub(super) async fn list_user_tokens_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let tokens = state
        .account
        .list_tokens(&actor.0)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!({ "data": tokens })))
}

pub(super) async fn create_user_token_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<CreateUserTokenRequest>,
) -> Result<Json<Value>, ApiError> {
    let token = state
        .account
        .create_token(&actor.0, &request.name)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!(token)))
}

pub(super) async fn delete_user_token_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(token_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .account
        .revoke_token(&actor.0, &token_id)
        .await
        .map_err(map_auth_error)?;
    Ok(Json(json!({ "success": true })))
}

pub(super) async fn list_admin_users_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let users = state
        .account
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
        .account
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
        .account
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
        .account
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
