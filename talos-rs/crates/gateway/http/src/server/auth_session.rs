use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use talos_app_protocol::requests::{
    AuthSessionReadResponse, AuthSessionSnapshot, AuthSessionStoreParams, AuthSessionUserSnapshot,
};
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse, CloudSessionCache, CloudSessionCacheEntry,
    CloudSessionUserSnapshot,
};

use super::errors::ApiError;
use super::state::HttpApiState;
use talos_auth::RequestIdentity;

pub(super) async fn read_auth_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<AuthSessionReadResponse>, ApiError> {
    let actor_token = match actor.access_token.as_deref() {
        Some(token) => token,
        None => return Ok(Json(AuthSessionReadResponse { session: None })),
    };
    let session = state
        .cloud_session_cache
        .read_candidate(CloudSessionCache::now_ms())
        .map_err(ApiError::from_talos_error)?
        .filter(|entry| entry.access_token == actor_token)
        .map(auth_session_snapshot_from_cache_entry);
    Ok(Json(AuthSessionReadResponse { session }))
}

pub(super) async fn start_cloud_login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<CloudLoginStartRequest>,
) -> Result<Json<CloudLoginStartResponse>, ApiError> {
    Ok(Json(
        state
            .cloud_login
            .start(request)
            .await
            .map_err(ApiError::from_talos_error)?,
    ))
}

pub(super) async fn cloud_login_status_handler(
    State(state): State<Arc<HttpApiState>>,
    axum::extract::Query(request): axum::extract::Query<CloudLoginStatusRequest>,
) -> Result<Json<CloudLoginStatusResponse>, ApiError> {
    Ok(Json(
        state
            .cloud_login
            .status(request)
            .await
            .map_err(ApiError::from_talos_error)?,
    ))
}

pub(super) async fn cancel_cloud_login_handler(
    State(state): State<Arc<HttpApiState>>,
    Json(request): Json<CloudLoginCancelRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .cloud_login
        .cancel(request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub(super) async fn store_auth_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Json(params): Json<AuthSessionStoreParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor_token = actor.access_token.as_deref().ok_or_else(|| {
        ApiError::from_talos_error(talos_error::TalosError::AuthError(
            "cloud session store requires verified cloud actor credentials".to_string(),
        ))
    })?;
    if actor_token != params.session.access_token {
        return Err(ApiError::from_talos_error(
            talos_error::TalosError::AuthError(
                "cloud session token does not match actor credentials".to_string(),
            ),
        ));
    }
    let entry =
        auth_session_cache_entry_from_snapshot(params.session, CloudSessionCache::now_ms())?;
    state
        .cloud_session_cache
        .store_verified_session(&entry)
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

pub(super) async fn clear_auth_session_handler(
    State(state): State<Arc<HttpApiState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .cloud_session_cache
        .clear()
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

fn auth_session_snapshot_from_cache_entry(entry: CloudSessionCacheEntry) -> AuthSessionSnapshot {
    AuthSessionSnapshot {
        schema_version: entry.schema_version,
        cloud_api_base_url: entry.cloud_api_base_url,
        access_token: entry.access_token,
        token_type: entry.token_type,
        expires_at_ms: entry.expires_at_ms.map(|value| value.to_string()),
        last_used_at_ms: entry.last_used_at_ms.to_string(),
        user: AuthSessionUserSnapshot {
            user_id: entry.user.user_id,
            username: entry.user.username,
            email: entry.user.email,
            role: entry.user.role,
        },
    }
}

fn auth_session_cache_entry_from_snapshot(
    snapshot: AuthSessionSnapshot,
    last_used_at_ms: i64,
) -> Result<CloudSessionCacheEntry, ApiError> {
    Ok(CloudSessionCacheEntry {
        schema_version: snapshot.schema_version,
        cloud_api_base_url: snapshot.cloud_api_base_url,
        access_token: snapshot.access_token,
        token_type: snapshot.token_type,
        expires_at_ms: parse_optional_i64(snapshot.expires_at_ms.as_deref(), "expires_at_ms")?,
        last_used_at_ms,
        user: CloudSessionUserSnapshot {
            user_id: snapshot.user.user_id,
            username: snapshot.user.username,
            email: snapshot.user.email,
            role: snapshot.user.role,
        },
    })
}

fn parse_optional_i64(value: Option<&str>, field: &str) -> Result<Option<i64>, ApiError> {
    value.map(|value| parse_i64(value, field)).transpose()
}

fn parse_i64(value: &str, field: &str) -> Result<i64, ApiError> {
    value.parse::<i64>().map_err(|err| {
        ApiError::from_talos_error(talos_error::TalosError::ValidationError(format!(
            "invalid cloud session {field}: {err}"
        )))
    })
}
