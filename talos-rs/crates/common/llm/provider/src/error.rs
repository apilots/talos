use talos_error::{ErrorCode, TalosError};

/// Map an upstream HTTP rejection without retaining its response body or URL.
/// Provider credentials are server configuration, not the requesting user's identity.
pub fn http_response_error(provider: &'static str, response: &reqwest::Response) -> TalosError {
    let status = response.status().as_u16();
    let (code, reason) = match status {
        401 | 403 => (
            ErrorCode::ProviderUnavailable,
            "the provider rejected its configured credentials or access",
        ),
        404 => (
            ErrorCode::ProviderUnavailable,
            "the configured endpoint or model was not found",
        ),
        408 | 504 => (ErrorCode::Timeout, "the provider request timed out"),
        429 => (
            ErrorCode::UsageLimitExceeded,
            "the provider rate or usage limit was exceeded",
        ),
        502 | 503 => (
            ErrorCode::ProviderUnavailable,
            "the provider is temporarily unavailable",
        ),
        _ => (
            ErrorCode::InternalError,
            "the provider rejected the model request; response details were withheld",
        ),
    };
    let message = format!("{provider} request failed (HTTP {status}): {reason}.");
    match response.error_for_status_ref() {
        Err(error) => TalosError::from_source(code, message, error.without_url()),
        Ok(_) => TalosError::new(code, message),
    }
}

/// Project recognized upstream stream codes, excluding arbitrary remote messages.
#[track_caller]
pub(crate) fn stream_response_error(error_type: Option<&str>) -> TalosError {
    let (code, message) = match error_type {
        Some("rate_limit_error" | "rate_limit_exceeded") => (ErrorCode::UsageLimitExceeded,
            "The model response stream failed because the provider rate limit was exceeded."),
        Some("overloaded_error" | "server_overloaded") => (ErrorCode::ProviderUnavailable,
            "The model response stream failed because the provider is overloaded."),
        Some("context_length_exceeded") => (ErrorCode::ContextWindowExceeded,
            "The model response stream failed because the context window was exceeded."),
        Some("authentication_error" | "invalid_api_key") => (ErrorCode::ProviderUnavailable,
            "The model response stream failed because the provider rejected its configured credentials."),
        _ => (ErrorCode::InternalError,
            "The provider reported a model response stream failure; remote details were withheld."),
    };
    TalosError::new(code, message)
}
