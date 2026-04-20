use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use talos_auth::RequestIdentity;

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
    if is_public_route(path.as_str()) {
        return Ok(next.run(req).await);
    }

    if state.auth_disabled {
        req.extensions_mut()
            .insert(RequestIdentity::mock_admin("mock-auth"));
        return Ok(next.run(req).await);
    }

    let auth_header = req.headers().get(axum::http::header::AUTHORIZATION);
    let token = match auth_header.and_then(|v| v.to_str().ok()) {
        Some(v) if v.starts_with("Bearer ") => v[7..].to_string(),
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    let auth_result = if state
        .static_token
        .as_ref()
        .is_some_and(|configured| configured == &token)
    {
        RequestIdentity::mock_admin("static-token")
    } else {
        state
            .auth_service
            .verify_access_token(&token)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    };

    req.extensions_mut().insert(auth_result);
    Ok(next.run(req).await)
}

fn is_public_route(path: &str) -> bool {
    matches!(path, "/health" | "/v1/auth/register" | "/v1/auth/login")
}
