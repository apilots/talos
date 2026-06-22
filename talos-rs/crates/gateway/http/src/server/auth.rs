use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use talos_auth::{CloudSessionCache, RequestIdentity};

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn lifecycle_gate_middleware(
    State(state): State<Arc<HttpApiState>>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if req.uri().path() == "/health" {
        return Ok(next.run(req).await);
    }

    if state.lifecycle.is_quiescing_or_later() {
        return Ok(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "server_shutting_down",
            "server is shutting down",
        )
        .into_response());
    }

    Ok(next.run(req).await)
}

pub(super) async fn auth_middleware(
    State(state): State<Arc<HttpApiState>>,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    if is_public_route(path.as_str()) || is_webui_route(path.as_str()) {
        return Ok(next.run(req).await);
    }

    // auth_disabled is used for local/integration testing and bypasses all auth checks.
    if state.auth_disabled {
        req.extensions_mut()
            .insert(RequestIdentity::auth_disabled());
        return Ok(next.run(req).await);
    }

    let auth_header = req.headers().get(axum::http::header::AUTHORIZATION);
    let token = match auth_header.and_then(|v| v.to_str().ok()) {
        Some(v) if v.starts_with("Bearer ") => v[7..].to_string(),
        _ => match state
            .app
            .account
            .read_cached_access_token(CloudSessionCache::now_ms())
        {
            Ok(Some(token)) => token,
            _ => return Err(StatusCode::UNAUTHORIZED),
        },
    };

    let skip_cache_touch = is_session_cache_management_route(path.as_str());
    let auth_result = match state.auth_service.verify_access_token(&token).await {
        Ok(identity) => identity,
        Err(_) => {
            if !skip_cache_touch {
                let _ = state
                    .app
                    .account
                    .clear_rejected_access_token(&token, CloudSessionCache::now_ms());
            }
            return Err(StatusCode::UNAUTHORIZED);
        }
    };
    if !skip_cache_touch {
        let _ = state
            .app
            .account
            .record_verified_access_token_use(&token, CloudSessionCache::now_ms());
    }

    req.extensions_mut().insert(auth_result);
    Ok(next.run(req).await)
}

fn is_public_route(path: &str) -> bool {
    path == "/health" || is_removed_local_auth_path(path) || is_cloud_login_route(path)
}

fn is_webui_route(path: &str) -> bool {
    !path.starts_with("/v1/")
}

fn is_removed_local_auth_path(path: &str) -> bool {
    matches!(
        path,
        concat!("/v1/auth/", "register") | concat!("/v1/auth/", "login")
    )
}

fn is_cloud_login_route(path: &str) -> bool {
    matches!(
        path,
        "/v1/auth/login/start" | "/v1/auth/login/status" | "/v1/auth/login/cancel"
    )
}

fn is_session_cache_management_route(path: &str) -> bool {
    path == "/v1/auth/session"
}
