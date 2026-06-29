use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use talos_config::{WebSearchProviderConfig, WebSearchProviderRuntimeConfig, WebSearchToolConfig};
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
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub recency_days: Option<u32>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WebSearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    #[serde(default)]
    pub source_domain: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub provider_rank: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WebSearchResponse {
    pub query: String,
    pub provider: WebSearchProviderConfig,
    pub recency_days: Option<u32>,
    pub freshness_applied: bool,
    pub summary: String,
    pub hits: Vec<WebSearchHit>,
}

#[async_trait]
trait SearchProvider: Send + Sync {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<WebSearchResponse>;
}

pub async fn run(request: &WebSearchRequest) -> TalosResult<WebSearchResponse> {
    run_with_config(request, &WebSearchToolConfig::default()).await
}

pub async fn run_with_config(
    request: &WebSearchRequest,
    config: &WebSearchToolConfig,
) -> TalosResult<WebSearchResponse> {
    validate_request(request)?;
    provider_for_config(config)?.search(request).await
}

fn provider_for_config(config: &WebSearchToolConfig) -> TalosResult<Box<dyn SearchProvider>> {
    match config.provider {
        WebSearchProviderConfig::DuckDuckGoInstantAnswer => Ok(Box::new(
            DuckDuckGoInstantAnswerProvider::new(config.duckduckgo.clone()),
        )),
        WebSearchProviderConfig::Brave => {
            Ok(Box::new(BraveSearchProvider::new(config.brave.clone())?))
        }
        WebSearchProviderConfig::Tavily => {
            Ok(Box::new(TavilySearchProvider::new(config.tavily.clone())?))
        }
    }
}

fn validate_request(request: &WebSearchRequest) -> TalosResult<()> {
    if request.query.trim().is_empty() {
        return Err(TalosError::ToolError("query must not be empty".to_string()));
    }
    if request.limit == 0 {
        return Err(TalosError::ToolError(
            "limit must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

#[derive(Clone)]
struct DuckDuckGoInstantAnswerProvider {
    config: WebSearchProviderRuntimeConfig,
}

impl DuckDuckGoInstantAnswerProvider {
    fn new(config: WebSearchProviderRuntimeConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGoInstantAnswerProvider {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<WebSearchResponse> {
        let client = build_client(self.config.timeout_ms)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.duckduckgo.com/");
        let body: Value = client
            .get(base_url)
            .query(&[
                ("q", request.query.trim()),
                ("format", "json"),
                ("no_redirect", "1"),
                ("no_html", "1"),
                ("skip_disambig", "1"),
            ])
            .send()
            .await
            .map_err(|err| TalosError::ToolError(format!("search web query: {err}")))?
            .error_for_status()
            .map_err(|err| TalosError::ToolError(format!("web_search failed: {err}")))?
            .json()
            .await
            .map_err(|err| TalosError::ToolError(format!("parse web search response: {err}")))?;
        let allowed = effective_allowed_domains(request);
        let hits = parse_duckduckgo_results(
            &body,
            allowed.as_slice(),
            request.blocked_domains.as_slice(),
            request.limit,
        )?;
        Ok(build_response(
            request,
            WebSearchProviderConfig::DuckDuckGoInstantAnswer,
            false,
            hits,
        ))
    }
}

#[derive(Clone)]
struct BraveSearchProvider {
    config: WebSearchProviderRuntimeConfig,
    api_key: String,
}

impl BraveSearchProvider {
    fn new(config: WebSearchProviderRuntimeConfig) -> TalosResult<Self> {
        let api_key = resolve_api_key(&config, "brave")?;
        Ok(Self { config, api_key })
    }
}

#[async_trait]
impl SearchProvider for BraveSearchProvider {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<WebSearchResponse> {
        let client = build_client(self.config.timeout_ms)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.search.brave.com/res/v1/web/search");
        let mut query = vec![
            ("q".to_string(), request.query.trim().to_string()),
            ("count".to_string(), request.limit.min(20).to_string()),
        ];
        if let Some(freshness) = brave_freshness(request.recency_days) {
            query.push(("freshness".to_string(), freshness.to_string()));
        }
        let body: Value = client
            .get(base_url)
            .header("X-Subscription-Token", &self.api_key)
            .query(&query)
            .send()
            .await
            .map_err(|err| TalosError::ToolError(format!("search web query: {err}")))?
            .error_for_status()
            .map_err(|err| TalosError::ToolError(format!("web_search failed: {err}")))?
            .json()
            .await
            .map_err(|err| TalosError::ToolError(format!("parse web search response: {err}")))?;
        let hits = parse_brave_results(&body, request)?;
        Ok(build_response(
            request,
            WebSearchProviderConfig::Brave,
            request.recency_days.is_some(),
            hits,
        ))
    }
}

#[derive(Clone)]
struct TavilySearchProvider {
    config: WebSearchProviderRuntimeConfig,
    api_key: String,
}

impl TavilySearchProvider {
    fn new(config: WebSearchProviderRuntimeConfig) -> TalosResult<Self> {
        let api_key = resolve_api_key(&config, "tavily")?;
        Ok(Self { config, api_key })
    }
}

#[async_trait]
impl SearchProvider for TavilySearchProvider {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<WebSearchResponse> {
        let client = build_client(self.config.timeout_ms)?;
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.tavily.com/search");
        let mut payload = serde_json::json!({
            "api_key": self.api_key,
            "query": request.query.trim(),
            "max_results": request.limit.min(20),
            "include_domains": effective_allowed_domains(request),
            "exclude_domains": request.blocked_domains,
            "include_answer": false,
            "include_raw_content": false,
        });
        if let Some(time_range) = tavily_time_range(request.recency_days) {
            payload["time_range"] = Value::String(time_range.to_string());
        }
        let body: Value = client
            .post(base_url)
            .json(&payload)
            .send()
            .await
            .map_err(|err| TalosError::ToolError(format!("search web query: {err}")))?
            .error_for_status()
            .map_err(|err| TalosError::ToolError(format!("web_search failed: {err}")))?
            .json()
            .await
            .map_err(|err| TalosError::ToolError(format!("parse web search response: {err}")))?;
        let hits = parse_tavily_results(&body, request)?;
        Ok(build_response(
            request,
            WebSearchProviderConfig::Tavily,
            request.recency_days.is_some(),
            hits,
        ))
    }
}

fn build_client(timeout_ms: u64) -> TalosResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(DEFAULT_WEB_USER_AGENT)
        .timeout(Duration::from_millis(timeout_ms.max(1)))
        .build()
        .map_err(|err| TalosError::ToolError(format!("build web search client: {err}")))
}

fn resolve_api_key(config: &WebSearchProviderRuntimeConfig, provider: &str) -> TalosResult<String> {
    if let Some(value) = config
        .api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(value.to_string());
    }
    if let Some(env_name) = config
        .api_key_env
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let value = std::env::var(env_name).map_err(|_| {
            TalosError::InvalidConfiguration(format!(
                "web search provider `{provider}` requires API key env `{env_name}`"
            ))
        })?;
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }
    Err(TalosError::InvalidConfiguration(format!(
        "web search provider `{provider}` requires an API key"
    )))
}

fn build_response(
    request: &WebSearchRequest,
    provider: WebSearchProviderConfig,
    freshness_applied: bool,
    hits: Vec<WebSearchHit>,
) -> WebSearchResponse {
    let summary = if hits.is_empty() {
        "No search results matched the requested filters.".to_string()
    } else {
        hits.iter()
            .enumerate()
            .map(|(index, hit)| format!("{}. {} ({})", index + 1, hit.title, hit.url))
            .collect::<Vec<_>>()
            .join("\n")
    };
    WebSearchResponse {
        query: request.query.trim().to_string(),
        provider,
        recency_days: request.recency_days,
        freshness_applied,
        summary,
        hits,
    }
}

fn effective_allowed_domains(request: &WebSearchRequest) -> Vec<String> {
    request
        .allowed_domains
        .iter()
        .chain(request.domains.iter())
        .cloned()
        .collect()
}

fn parse_duckduckgo_results(
    body: &Value,
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

    push_duckduckgo_abstract_hit(body, &mut hits, &allowed, &blocked)?;
    collect_duckduckgo_topic_hits(
        body.get("Results").and_then(Value::as_array),
        &mut hits,
        &allowed,
        &blocked,
        limit,
    )?;
    collect_duckduckgo_topic_hits(
        body.get("RelatedTopics").and_then(Value::as_array),
        &mut hits,
        &allowed,
        &blocked,
        limit,
    )?;
    hits.truncate(limit);
    rank_hits(&mut hits);
    Ok(hits)
}

fn push_duckduckgo_abstract_hit(
    body: &Value,
    hits: &mut Vec<WebSearchHit>,
    allowed: &[String],
    blocked: &[String],
) -> TalosResult<()> {
    let Some(url) = body.get("AbstractURL").and_then(Value::as_str) else {
        return Ok(());
    };
    if url.trim().is_empty() || !domain_allowed(url, allowed, blocked)? {
        return Ok(());
    }
    let title = body
        .get("Heading")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(url);
    let snippet = body
        .get("AbstractText")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (snippet, _) = truncate_chars(snippet, 280);
    hits.push(WebSearchHit {
        title: title.to_string(),
        url: url.to_string(),
        snippet,
        source_domain: domain_for_url(url),
        published_at: None,
        provider_rank: None,
    });
    Ok(())
}

fn collect_duckduckgo_topic_hits(
    items: Option<&Vec<Value>>,
    hits: &mut Vec<WebSearchHit>,
    allowed: &[String],
    blocked: &[String],
    limit: usize,
) -> TalosResult<()> {
    let Some(items) = items else {
        return Ok(());
    };
    for item in items {
        if hits.len() >= limit {
            return Ok(());
        }
        if let Some(children) = item.get("Topics").and_then(Value::as_array) {
            collect_duckduckgo_topic_hits(Some(children), hits, allowed, blocked, limit)?;
            continue;
        }
        let Some(url) = item.get("FirstURL").and_then(Value::as_str) else {
            continue;
        };
        if !domain_allowed(url, allowed, blocked)? {
            continue;
        }
        let text = item.get("Text").and_then(Value::as_str).unwrap_or(url);
        let (title, snippet) = split_title_snippet(text);
        let (snippet, _) = truncate_chars(snippet, 280);
        hits.push(WebSearchHit {
            title: title.to_string(),
            url: url.to_string(),
            snippet,
            source_domain: domain_for_url(url),
            published_at: None,
            provider_rank: None,
        });
    }
    Ok(())
}

fn parse_brave_results(body: &Value, request: &WebSearchRequest) -> TalosResult<Vec<WebSearchHit>> {
    let allowed = normalized_allowed(request);
    let blocked = normalized_blocked(request);
    let mut hits = Vec::new();
    let items = body
        .get("web")
        .and_then(|web| web.get("results"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in items.into_iter().take(request.limit) {
        let Some(url) = item.get("url").and_then(Value::as_str) else {
            continue;
        };
        if !domain_allowed(url, &allowed, &blocked)? {
            continue;
        }
        let title = item.get("title").and_then(Value::as_str).unwrap_or(url);
        let description = item
            .get("description")
            .or_else(|| item.get("snippet"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (snippet, _) = truncate_chars(description, 280);
        hits.push(WebSearchHit {
            title: title.to_string(),
            url: url.to_string(),
            snippet,
            source_domain: domain_for_url(url),
            published_at: item
                .get("age")
                .or_else(|| item.get("page_age"))
                .and_then(Value::as_str)
                .map(str::to_string),
            provider_rank: None,
        });
    }
    rank_hits(&mut hits);
    Ok(hits)
}

fn parse_tavily_results(
    body: &Value,
    request: &WebSearchRequest,
) -> TalosResult<Vec<WebSearchHit>> {
    let allowed = normalized_allowed(request);
    let blocked = normalized_blocked(request);
    let mut hits = Vec::new();
    let items = body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in items.into_iter().take(request.limit) {
        let Some(url) = item.get("url").and_then(Value::as_str) else {
            continue;
        };
        if !domain_allowed(url, &allowed, &blocked)? {
            continue;
        }
        let title = item.get("title").and_then(Value::as_str).unwrap_or(url);
        let content = item
            .get("content")
            .or_else(|| item.get("snippet"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (snippet, _) = truncate_chars(content, 280);
        hits.push(WebSearchHit {
            title: title.to_string(),
            url: url.to_string(),
            snippet,
            source_domain: domain_for_url(url),
            published_at: item
                .get("published_date")
                .or_else(|| item.get("published_at"))
                .and_then(Value::as_str)
                .map(str::to_string),
            provider_rank: None,
        });
    }
    rank_hits(&mut hits);
    Ok(hits)
}

fn normalized_allowed(request: &WebSearchRequest) -> Vec<String> {
    effective_allowed_domains(request)
        .iter()
        .map(|item| normalize_domain(item))
        .collect()
}

fn normalized_blocked(request: &WebSearchRequest) -> Vec<String> {
    request
        .blocked_domains
        .iter()
        .map(|item| normalize_domain(item))
        .collect()
}

fn domain_allowed(url: &str, allowed: &[String], blocked: &[String]) -> TalosResult<bool> {
    let parsed = parse_http_url(url)?;
    let Some(domain) = url_domain(&parsed) else {
        return Ok(false);
    };
    if !allowed.is_empty()
        && !allowed
            .iter()
            .any(|item| domain == *item || domain.ends_with(&format!(".{item}")))
    {
        return Ok(false);
    }
    if blocked
        .iter()
        .any(|item| domain == *item || domain.ends_with(&format!(".{item}")))
    {
        return Ok(false);
    }
    Ok(true)
}

fn domain_for_url(url: &str) -> Option<String> {
    parse_http_url(url).ok().and_then(|url| url_domain(&url))
}

fn split_title_snippet(text: &str) -> (&str, &str) {
    text.split_once(" - ").unwrap_or((text, text))
}

fn rank_hits(hits: &mut [WebSearchHit]) {
    for (index, hit) in hits.iter_mut().enumerate() {
        hit.provider_rank = Some(index + 1);
    }
}

fn brave_freshness(recency_days: Option<u32>) -> Option<&'static str> {
    match recency_days? {
        0 | 1 => Some("pd"),
        2..=7 => Some("pw"),
        8..=31 => Some("pm"),
        _ => Some("py"),
    }
}

fn tavily_time_range(recency_days: Option<u32>) -> Option<&'static str> {
    match recency_days? {
        0 | 1 => Some("day"),
        2..=7 => Some("week"),
        8..=31 => Some("month"),
        _ => Some("year"),
    }
}

#[cfg(test)]
#[path = "web_search_tests.rs"]
mod tests;
