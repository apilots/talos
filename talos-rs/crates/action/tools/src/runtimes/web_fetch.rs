use serde::{Deserialize, Serialize};
use std::time::Duration;
use talos_error::{ErrorCode, TalosError, TalosResult};

use crate::runtimes::web_common::{
    extract_html_title, html_to_text, parse_http_url, truncate_chars, DEFAULT_WEB_USER_AGENT,
    TRUNCATION_MARKER,
};

const DEFAULT_MAX_CHARS: usize = 12_000;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

fn default_max_chars() -> usize {
    DEFAULT_MAX_CHARS
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebFetchRequest {
    pub url: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WebFetchResponse {
    pub requested_url: String,
    pub final_url: String,
    pub redirected: bool,
    pub title: Option<String>,
    pub mime_type: Option<String>,
    pub content: String,
    pub truncated: bool,
    pub auth_warning: Option<String>,
    pub prompt: Option<String>,
}

pub async fn run(request: &WebFetchRequest) -> TalosResult<WebFetchResponse> {
    if request.max_chars == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "max_chars must be greater than zero".to_string(),
        ));
    }

    let url = parse_http_url(&request.url)?;
    let auth_warning = private_url_warning(&url);
    let client = reqwest::Client::builder()
        .user_agent(DEFAULT_WEB_USER_AGENT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| {
            TalosError::from_source(
                ErrorCode::ToolExecutionFailed,
                "Build web client failed.",
                err.without_url(),
            )
        })?;
    let mut response = client.get(url.clone()).send().await.map_err(|err| {
        TalosError::from_source(
            ErrorCode::ToolExecutionFailed,
            "Fetch web content failed.",
            err.without_url(),
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("web_fetch received non-success status: {status}"),
        ));
    }

    let final_url = response.url().to_string();
    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim().to_string());
    let mut bytes = Vec::new();
    let mut body_truncated = false;
    while let Some(chunk) = response.chunk().await.map_err(|err| {
        TalosError::from_source(
            ErrorCode::ToolExecutionFailed,
            "Read web content failed.",
            err.without_url(),
        )
    })? {
        let remaining = MAX_RESPONSE_BYTES - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if chunk.len() > remaining {
            body_truncated = true;
            break;
        }
    }
    // Keep reqwest's charset, BOM and malformed-sequence decoding over bounded bytes.
    let mut response = http::Response::<reqwest::Body>::from(response);
    *response.body_mut() = bytes.into();
    let body = reqwest::Response::from(response)
        .text()
        .await
        .map_err(|err| {
            TalosError::from_source(
                ErrorCode::ToolExecutionFailed,
                "Decode web content failed.",
                err.without_url(),
            )
        })?;
    let (title, extracted_text) = match mime_type.as_deref() {
        Some(content_type) if content_type.contains("html") => {
            (extract_html_title(&body), html_to_text(&body))
        }
        _ => (None, body),
    };
    let (mut content, truncated) = truncate_chars(extracted_text.trim(), request.max_chars);
    if body_truncated && !truncated {
        content.push_str(TRUNCATION_MARKER);
    }

    Ok(WebFetchResponse {
        requested_url: request.url.clone(),
        redirected: final_url != request.url,
        final_url,
        title,
        mime_type,
        content,
        truncated: truncated || body_truncated,
        auth_warning,
        prompt: request.prompt.clone(),
    })
}

fn private_url_warning(url: &reqwest::Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || private_172_range(&host)
    {
        Some("URL appears to target a private or local network address".to_string())
    } else {
        None
    }
}

fn private_172_range(host: &str) -> bool {
    let Some(rest) = host.strip_prefix("172.") else {
        return false;
    };
    let Some(second) = rest.split('.').next() else {
        return false;
    };
    second
        .parse::<u8>()
        .map(|value| (16..=31).contains(&value))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "web_fetch_tests.rs"]
mod tests;
