use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use talos_auth::RequestIdentity;

use super::errors::ApiError;
use super::state::HttpApiState;
use talos_error::ErrorCode;

pub(super) async fn lifecycle_gate_middleware(
    State(state): State<Arc<HttpApiState>>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if req.uri().path() == "/health" {
        return Ok(next.run(req).await);
    }

    if state.lifecycle.is_quiescing_or_later() {
        return Ok(ApiError::new(ErrorCode::ServiceUnavailable).into_response());
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

    let token = match req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        Some(value) if value.starts_with("Bearer ") => value[7..].to_string(),
        _ => {
            return Ok(ApiError::new(ErrorCode::Unauthenticated).into_response());
        }
    };

    let auth_result = match state.auth_service.verify_access_token(&token).await {
        Ok(identity) => identity,
        Err(error) => {
            return Ok(ApiError::from_talos_error(error).into_response());
        }
    };

    req.extensions_mut().insert(auth_result);
    Ok(next.run(req).await)
}

fn is_public_route(path: &str) -> bool {
    path == "/health" || is_cloud_login_route(path)
}

fn is_webui_route(path: &str) -> bool {
    !path.starts_with("/v1/")
}

fn is_cloud_login_route(path: &str) -> bool {
    matches!(
        path,
        "/v1/auth/login/start" | "/v1/auth/login/status" | "/v1/auth/login/cancel"
    )
}
