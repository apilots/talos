use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_config::{RuntimeEnv, WebSearchProviderConfig, WebSearchToolConfig};
use talos_error::{TalosError, TalosResult};

use crate::runtimes::web_common::{normalize_domain, parse_http_url, truncate_chars, url_domain};

mod brave;
mod firecrawl;
mod tavily;

const DEFAULT_RESULT_LIMIT: usize = 5;
const MAX_RESULT_LIMIT: usize = 20;

fn default_limit() -> usize {
    DEFAULT_RESULT_LIMIT
}

#[derive(Debug, Clone, Deserialize)]
/// Provider-neutral input accepted by the Web Search runtime.
pub struct WebSearchRequest {
    pub query: String,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
    #[serde(default)]
    pub recency_days: Option<u32>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// One normalized web result returned to all Talos callers.
pub struct WebSearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source_domain: Option<String>,
    pub rank: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Deterministic Web Search output independent of provider wire formats.
pub struct WebSearchResponse {
    pub query: String,
    pub provider: WebSearchProviderConfig,
    pub recency_days: Option<u32>,
    pub freshness_applied: bool,
    pub hits: Vec<WebSearchHit>,
}

#[derive(Debug)]
struct SearchCandidate {
    title: String,
    url: String,
    snippet: String,
}

#[async_trait]
/// Private boundary implemented by each configured search provider adapter.
trait SearchProvider: Send + Sync {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<Vec<SearchCandidate>>;

    fn supports_recency(&self) -> bool;
}

pub async fn run_with_config(
    request: &WebSearchRequest,
    config: &WebSearchToolConfig,
    runtime_env: &RuntimeEnv,
) -> TalosResult<WebSearchResponse> {
    validate_request(request)?;
    let provider = provider_for_config(config, runtime_env)?;
    let freshness_applied = request.recency_days.is_some() && provider.supports_recency();
    let candidates = provider.search(request).await?;
    let hits = finalize_hits(request, candidates)?;

    Ok(WebSearchResponse {
        query: request.query.trim().to_string(),
        provider: config.provider,
        recency_days: request.recency_days,
        freshness_applied,
        hits,
    })
}

fn provider_for_config(
    config: &WebSearchToolConfig,
    runtime_env: &RuntimeEnv,
) -> TalosResult<Box<dyn SearchProvider>> {
    match config.provider {
        WebSearchProviderConfig::Firecrawl => Ok(Box::new(
            firecrawl::FirecrawlSearchProvider::new(config.firecrawl.clone(), runtime_env)?,
        )),
        WebSearchProviderConfig::Brave => Ok(Box::new(brave::BraveSearchProvider::new(
            config.brave.clone(),
            runtime_env,
        )?)),
        WebSearchProviderConfig::Tavily => Ok(Box::new(tavily::TavilySearchProvider::new(
            config.tavily.clone(),
            runtime_env,
        )?)),
    }
}

fn validate_request(request: &WebSearchRequest) -> TalosResult<()> {
    if request.query.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "query must not be empty".to_string(),
        ));
    }
    if !(1..=MAX_RESULT_LIMIT).contains(&request.limit) {
        return Err(TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("limit must be between 1 and {MAX_RESULT_LIMIT}"),
        ));
    }
    Ok(())
}

fn finalize_hits(
    request: &WebSearchRequest,
    candidates: Vec<SearchCandidate>,
) -> TalosResult<Vec<WebSearchHit>> {
    let allowed = normalized_domains(&request.allowed_domains);
    let blocked = normalized_domains(&request.blocked_domains);
    let mut hits = Vec::new();

    for candidate in candidates {
        let parsed_url = parse_http_url(&candidate.url)?;
        let Some(source_domain) = url_domain(&parsed_url) else {
            continue;
        };
        if !domain_allowed(&source_domain, &allowed, &blocked) {
            continue;
        }
        let (snippet, _) = truncate_chars(&candidate.snippet, 280);
        hits.push(WebSearchHit {
            title: candidate.title,
            url: candidate.url,
            snippet,
            source_domain: Some(source_domain),
            rank: hits.len() + 1,
        });
        if hits.len() == request.limit {
            break;
        }
    }

    Ok(hits)
}

fn normalized_domains(domains: &[String]) -> Vec<String> {
    domains
        .iter()
        .map(|domain| normalize_domain(domain))
        .filter(|domain| !domain.is_empty())
        .collect()
}

fn domain_allowed(domain: &str, allowed: &[String], blocked: &[String]) -> bool {
    let matches =
        |candidate: &str| domain == candidate || domain.ends_with(&format!(".{candidate}"));
    (allowed.is_empty() || allowed.iter().any(|candidate| matches(candidate)))
        && !blocked.iter().any(|candidate| matches(candidate))
}

fn provider_result_limit(request: &WebSearchRequest) -> usize {
    if request.allowed_domains.is_empty() && request.blocked_domains.is_empty() {
        request.limit
    } else {
        MAX_RESULT_LIMIT
    }
}

fn configured_api_key(
    config: &talos_config::WebSearchProviderRuntimeConfig,
    runtime_env: &RuntimeEnv,
) -> Option<String> {
    config
        .api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            config
                .api_key_env
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .and_then(|name| runtime_env.get(name).map(str::to_string))
                .filter(|value| !value.trim().is_empty())
        })
}

fn resolve_api_key(
    config: &talos_config::WebSearchProviderRuntimeConfig,
    runtime_env: &RuntimeEnv,
    provider: &str,
) -> TalosResult<String> {
    configured_api_key(config, runtime_env).ok_or_else(|| {
        let requirement = config
            .api_key_env
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|env_name| format!(" API key env `{env_name}`"))
            .unwrap_or_else(|| " an API key".to_string());
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("web search provider `{provider}` requires{requirement}"),
        )
    })
}

fn brave_freshness(recency_days: Option<u32>) -> Option<&'static str> {
    match recency_days? {
        0 | 1 => Some("pd"),
        2..=7 => Some("pw"),
        8..=31 => Some("pm"),
        _ => Some("py"),
    }
}

fn firecrawl_time_range(recency_days: Option<u32>) -> Option<&'static str> {
    match recency_days? {
        0 | 1 => Some("qdr:d"),
        2..=7 => Some("qdr:w"),
        8..=31 => Some("qdr:m"),
        _ => Some("qdr:y"),
    }
}

#[cfg(test)]
#[path = "web_search_tests.rs"]
mod tests;
