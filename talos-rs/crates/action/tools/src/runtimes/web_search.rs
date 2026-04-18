use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::runtimes::web_common::{
    normalize_domain, parse_http_url, truncate_chars, url_domain, DEFAULT_WEB_USER_AGENT,
};

const DEFAULT_RESULT_LIMIT: usize = 5;

fn default_limit() -> usize {
    DEFAULT_RESULT_LIMIT
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebSearchRequest {
    pub query: String,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WebSearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WebSearchResponse {
    pub query: String,
    pub summary: String,
    pub hits: Vec<WebSearchHit>,
}

pub async fn run(request: &WebSearchRequest) -> TalosResult<WebSearchResponse> {
    let query = request.query.trim();
    if query.is_empty() {
        return Err(TalosError::ToolError("query must not be empty".to_string()));
    }
    if request.limit == 0 {
        return Err(TalosError::ToolError(
            "limit must be greater than zero".to_string(),
        ));
    }

    let client = reqwest::Client::builder()
        .user_agent(DEFAULT_WEB_USER_AGENT)
        .build()
        .map_err(|err| TalosError::ToolError(format!("build web search client: {err}")))?;
    let response = client
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        .send()
        .await
        .map_err(|err| TalosError::ToolError(format!("search web query: {err}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(TalosError::ToolError(format!(
            "web_search received non-success status: {status}"
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|err| TalosError::ToolError(format!("read web search response: {err}")))?;
    let hits = parse_search_results(
        &body,
        request.allowed_domains.as_slice(),
        request.blocked_domains.as_slice(),
        request.limit,
    )?;
    let summary = if hits.is_empty() {
        "No search results matched the requested filters.".to_string()
    } else {
        hits.iter()
            .enumerate()
            .map(|(index, hit)| format!("{}. {} ({})", index + 1, hit.title, hit.url))
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(WebSearchResponse {
        query: query.to_string(),
        summary,
        hits,
    })
}

fn parse_search_results(
    html: &str,
    allowed_domains: &[String],
    blocked_domains: &[String],
    limit: usize,
) -> TalosResult<Vec<WebSearchHit>> {
    let allowed: Vec<String> = allowed_domains
        .iter()
        .map(|item| normalize_domain(item))
        .collect();
    let blocked: Vec<String> = blocked_domains
        .iter()
        .map(|item| normalize_domain(item))
        .collect();
    let mut hits = Vec::new();
    let mut remaining = html;

    while hits.len() < limit {
        let Some(anchor_index) = remaining.find("result__a") else {
            break;
        };
        remaining = &remaining[anchor_index..];
        let Some(href_index) = remaining.find("href=\"") else {
            break;
        };
        let href_start = href_index + 6;
        let href_rest = &remaining[href_start..];
        let Some(href_end) = href_rest.find('"') else {
            break;
        };
        let raw_url = &href_rest[..href_end];
        let Some(anchor_close) = href_rest[href_end..].find('>') else {
            break;
        };
        let title_rest = &href_rest[href_end + anchor_close + 1..];
        let Some(title_end) = title_rest.find("</a>") else {
            break;
        };
        let title = strip_html(title_rest[..title_end].trim());
        let url = decode_duckduckgo_redirect(raw_url);
        let Some(snippet_index) = title_rest[title_end..].find("result__snippet") else {
            remaining = &title_rest[title_end..];
            continue;
        };
        let snippet_rest = &title_rest[title_end + snippet_index..];
        let Some(snippet_start) = snippet_rest.find('>') else {
            break;
        };
        let snippet_body = &snippet_rest[snippet_start + 1..];
        let Some(snippet_end) = snippet_body
            .find("</a>")
            .or_else(|| snippet_body.find("</div>"))
        else {
            break;
        };
        let snippet = strip_html(snippet_body[..snippet_end].trim());
        remaining = &snippet_body[snippet_end..];

        let parsed = parse_http_url(&url)?;
        let Some(domain) = url_domain(&parsed) else {
            continue;
        };
        if !allowed.is_empty()
            && !allowed
                .iter()
                .any(|item| domain == *item || domain.ends_with(&format!(".{item}")))
        {
            continue;
        }
        if blocked
            .iter()
            .any(|item| domain == *item || domain.ends_with(&format!(".{item}")))
        {
            continue;
        }

        let (snippet, _) = truncate_chars(&snippet, 280);
        hits.push(WebSearchHit {
            title,
            url,
            snippet,
        });
    }

    Ok(hits)
}

fn decode_duckduckgo_redirect(raw_url: &str) -> String {
    if let Ok(url) = reqwest::Url::parse(&format!("https://duckduckgo.com{raw_url}")) {
        if let Some(target) = url
            .query_pairs()
            .find(|(key, _)| key == "uddg")
            .map(|(_, value)| value.to_string())
        {
            return target;
        }
    }
    raw_url.to_string()
}

fn strip_html(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut inside_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => output.push(ch),
            _ => {}
        }
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_results_filters_domains() {
        let html = r#"
            <a class="result__a" href="https://www.rust-lang.org/tools">Rust Tools</a>
            <div class="result__snippet">Official tools page</div>
            <a class="result__a" href="https://spam.example/bad">Spam</a>
            <div class="result__snippet">Ignore me</div>
        "#;
        let hits = parse_search_results(
            html,
            &[String::from("rust-lang.org")],
            &[String::from("spam.example")],
            5,
        )
        .expect("search results should parse");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Rust Tools");
    }

    #[tokio::test]
    async fn web_search_rejects_empty_query() {
        let error = run(&WebSearchRequest {
            query: "   ".to_string(),
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            limit: 5,
        })
        .await
        .expect_err("empty query should fail");
        assert!(error.to_string().contains("query must not be empty"));
    }
}
