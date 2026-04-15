use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use talos_auth::{Auth, AuthResult, TokenType};

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
    if path == "/health" {
        return Ok(next.run(req).await);
    }

    if state.auth_disabled {
        req.extensions_mut().insert(mock_auth_result("mock-auth"));
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
        mock_auth_result("static-token")
    } else {
        state
            .auth
            .verify(&token)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    };

    if !auth_result.is_valid {
        return Err(StatusCode::UNAUTHORIZED);
    }

    req.extensions_mut().insert(auth_result);
    Ok(next.run(req).await)
}

fn mock_auth_result(user_id: &str) -> AuthResult {
    AuthResult {
        user_id: user_id.to_string(),
        expires_at: u64::MAX,
        is_valid: true,
        token_type: TokenType::Service,
        scopes: vec!["*".to_string()],
        allowed_channels: vec!["*".to_string()],
        act_as: Some("*".to_string()),
        issuer: Some("mock-auth".to_string()),
    }
}
