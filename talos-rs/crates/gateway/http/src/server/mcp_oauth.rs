//! Fixed browser callback; the service resolves recorded actor/node policy by OAuth state.
use super::{errors::ApiError, state::HttpApiState};
use agent_protocol::McpOAuthCallbackOutcome;
use axum::{
    extract::{RawQuery, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use talos_error::ErrorCode;

pub(super) async fn callback_handler(
    State(state): State<Arc<HttpApiState>>,
    RawQuery(query): RawQuery,
) -> Response {
    let result = async {
        let query = query
            .filter(|query| query.len() <= 8192)
            .ok_or_else(invalid)?;
        let mut state_value = None;
        let mut issuer = None;
        let mut code = None;
        let mut denial = None;
        for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
            let field = match name.as_ref() {
                "state" => &mut state_value,
                "iss" => &mut issuer,
                "code" => &mut code,
                "error" => &mut denial,
                _ => continue,
            };
            if value.is_empty() || field.replace(value.into_owned()).is_some() {
                return Err(invalid());
            }
        }
        let state_value = state_value
            .filter(|value| value.len() <= 4096)
            .ok_or_else(invalid)?;
        let outcome = match (code, denial) {
            (Some(code), None) if code.len() <= 4096 => {
                McpOAuthCallbackOutcome::AuthorizationCode(code)
            }
            (None, Some(code)) if code.len() <= 256 => {
                McpOAuthCallbackOutcome::ProviderErrorCode(code)
            }
            _ => return Err(invalid()),
        };
        state
            .app
            .complete_mcp_oauth_callback(state_value, issuer, outcome)
            .await
            .map_err(ApiError::from_talos_error)
    }
    .await;
    let mut response = match result {
        Ok(()) => "Authorization completed. Return to Talos.".into_response(),
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}
fn invalid() -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest)
}
