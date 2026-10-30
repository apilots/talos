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

/// Only protocol identifiers carry semantic rejection meaning, never remote text.
fn upstream_rejection(error_type: &str) -> Option<(ErrorCode, &'static str)> {
    Some(match error_type {
        "rate_limit_error" | "rate_limit_exceeded" => (
            ErrorCode::UsageLimitExceeded,
            "the provider rate limit was exceeded",
        ),
        "overloaded_error" | "server_overloaded" => {
            (ErrorCode::ProviderUnavailable, "the provider is overloaded")
        }
        "context_length_exceeded" => (
            ErrorCode::ContextWindowExceeded,
            "the context window was exceeded",
        ),
        "authentication_error" | "invalid_api_key" => (
            ErrorCode::ProviderUnavailable,
            "the provider rejected its configured credentials",
        ),
        _ => return None,
    })
}

/// Project recognized upstream stream codes, excluding arbitrary remote messages.
#[track_caller]
pub(crate) fn stream_response_error(error_type: Option<&str>) -> TalosError {
    match error_type.and_then(upstream_rejection) {
        Some((code, reason)) => TalosError::new(
            code,
            format!("The model response stream failed because {reason}."),
        ),
        None => TalosError::internal(
            "The provider reported a model response stream failure; remote details were withheld.",
        ),
    }
}

/// Preserve dependency timeout classification without leaking the request URL.
#[track_caller]
pub fn transport_error(operation: &'static str, error: reqwest::Error) -> TalosError {
    let (code, reason) = if error.is_timeout() {
        (ErrorCode::Timeout, "timed out")
    } else {
        (
            ErrorCode::InternalError,
            "failed; technical details were withheld",
        )
    };
    TalosError::from_source(code, format!("{operation} {reason}."), error.without_url())
}

/// Consume only a bounded rejection envelope. Arbitrary remote text is discarded.
pub async fn http_rejection_error(
    provider: &'static str,
    mut response: reqwest::Response,
) -> TalosError {
    let mut error = http_response_error(provider, &response);
    let status_source = response
        .error_for_status_ref()
        .err()
        .map(reqwest::Error::without_url);
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
        .map(str::to_owned);
    let read = async {
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > 8192 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<serde_json::Value>(&bytes).ok()
    };
    if let Ok(Some(body)) = tokio::time::timeout(std::time::Duration::from_secs(2), read).await {
        if let Some(envelope) = body.get("error") {
            let code = envelope.get("code").and_then(serde_json::Value::as_str);
            let kind = envelope.get("type").and_then(serde_json::Value::as_str);
            // These are protocol identifiers, never arbitrary provider messages.
            let recognized = [code, kind].into_iter().flatten().find_map(|identifier| {
                upstream_rejection(identifier).map(|rejection| (identifier, rejection))
            });
            if let Some((identifier, (code, reason))) = recognized {
                let status = response.status().as_u16();
                error = TalosError::new(
                    code,
                    format!("{provider} request failed (HTTP {status}): {reason}."),
                )
                .context(format!("upstream_code={identifier}"));
                if let Some(source) = status_source {
                    error = error.with_source(source);
                }
            }
            for (name, value) in [("code", code), ("type", kind)] {
                if let Some(value) = value.filter(|value| {
                    matches!(
                        *value,
                        "BadRequestError"
                            | "Bad Request"
                            | "invalid_request_error"
                            | "invalid_request"
                            | "unsupported_parameter"
                            | "invalid_parameter"
                    )
                }) {
                    error = error.context(format!("upstream_{name}={value}"));
                }
            }
            if let Some(param) = envelope
                .get("param")
                .and_then(serde_json::Value::as_str)
                .filter(|value| {
                    matches!(
                        *value,
                        "reasoning_effort"
                            | "reasoning.effort"
                            | "max_tokens"
                            | "max_completion_tokens"
                            | "messages"
                            | "model"
                            | "tools"
                            | "chat_template_kwargs"
                    )
                })
            {
                error = error.context(format!("upstream_param={param}"));
            }
        }
    }
    if let Some(request_id) = request_id {
        error = error.context(format!("upstream_request_id={request_id}"));
    }
    error
}

/// Private request identity travels with a response, never in an upstream body.
#[derive(Clone)]
pub(crate) struct ModelRequestContext(pub String);

pub(crate) fn response_context(response: &reqwest::Response) -> String {
    response
        .extensions()
        .get::<ModelRequestContext>()
        .map(|value| value.0.clone())
        .unwrap_or_default()
}
