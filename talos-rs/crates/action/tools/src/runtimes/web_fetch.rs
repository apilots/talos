use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::runtimes::web_common::{
    extract_html_title, html_to_text, parse_http_url, truncate_chars, DEFAULT_WEB_USER_AGENT,
};

const DEFAULT_MAX_CHARS: usize = 12_000;

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
        return Err(TalosError::ToolError(
            "max_chars must be greater than zero".to_string(),
        ));
    }

    let url = parse_http_url(&request.url)?;
    let auth_warning = private_url_warning(&url);
    let client = reqwest::Client::builder()
        .user_agent(DEFAULT_WEB_USER_AGENT)
        .build()
        .map_err(|err| TalosError::ToolError(format!("build web client: {err}")))?;
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|err| TalosError::ToolError(format!("fetch web content: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(TalosError::ToolError(format!(
            "web_fetch received non-success status: {status}"
        )));
    }

    let final_url = response.url().to_string();
    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim().to_string());
    let body = response
        .text()
        .await
        .map_err(|err| TalosError::ToolError(format!("read web content: {err}")))?;
    let (title, extracted_text) = match mime_type.as_deref() {
        Some(content_type) if content_type.contains("html") => {
            (extract_html_title(&body), html_to_text(&body))
        }
        _ => (None, body),
    };
    let (content, truncated) = truncate_chars(extracted_text.trim(), request.max_chars);

    Ok(WebFetchResponse {
        requested_url: request.url.clone(),
        redirected: final_url != request.url,
        final_url,
        title,
        mime_type,
        content,
        truncated,
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn web_fetch_rejects_zero_max_chars() {
        let error = run(&WebFetchRequest {
            url: "https://example.com".to_string(),
            prompt: None,
            max_chars: 0,
        })
        .await
        .expect_err("zero max chars should fail");
        assert!(error
            .to_string()
            .contains("max_chars must be greater than zero"));
    }

    #[test]
    fn web_fetch_warns_for_private_urls() {
        let url = reqwest::Url::parse("http://127.0.0.1:3000").expect("url should parse");
        assert!(private_url_warning(&url).is_some());
    }
}
