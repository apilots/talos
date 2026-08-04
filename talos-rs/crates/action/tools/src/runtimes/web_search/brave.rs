use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use talos_config::{RuntimeEnv, WebSearchProviderRuntimeConfig};
use talos_error::{TalosError, TalosResult};

use crate::runtimes::web_common::DEFAULT_WEB_USER_AGENT;

use super::{
    brave_freshness, provider_result_limit, resolve_api_key, SearchCandidate, SearchProvider,
    WebSearchRequest,
};

pub(super) struct BraveSearchProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl BraveSearchProvider {
    pub(super) fn new(
        config: WebSearchProviderRuntimeConfig,
        runtime_env: &RuntimeEnv,
    ) -> TalosResult<Self> {
        let api_key = resolve_api_key(&config, runtime_env, "brave")?;
        let base_url = config
            .base_url
            .unwrap_or_else(|| "https://api.search.brave.com/res/v1/web/search".to_string());
        let client = reqwest::Client::builder()
            .user_agent(DEFAULT_WEB_USER_AGENT)
            .timeout(Duration::from_millis(config.timeout_ms.max(1)))
            .build()
            .map_err(|err| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("build Brave search client: {err}"),
                )
            })?;
        Ok(Self {
            api_key,
            base_url,
            client,
        })
    }
}

#[async_trait]
impl SearchProvider for BraveSearchProvider {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<Vec<SearchCandidate>> {
        let mut query = vec![
            ("q", request.query.trim().to_string()),
            ("count", provider_result_limit(request).to_string()),
        ];
        if let Some(freshness) = brave_freshness(request.recency_days) {
            query.push(("freshness", freshness.to_string()));
        }
        let response: BraveSearchResponse = self
            .client
            .get(&self.base_url)
            .header("X-Subscription-Token", &self.api_key)
            .query(&query)
            .send()
            .await
            .map_err(|err| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("Brave search failed: {err}"),
                )
            })?
            .error_for_status()
            .map_err(|err| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("Brave search failed: {err}"),
                )
            })?
            .json()
            .await
            .map_err(|err| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("parse Brave search response: {err}"),
                )
            })?;

        Ok(response
            .web
            .map(|web| web.results)
            .unwrap_or_default()
            .into_iter()
            .map(|result| SearchCandidate {
                title: result.title,
                url: result.url,
                snippet: result.description,
            })
            .collect())
    }

    fn supports_recency(&self) -> bool {
        true
    }
}

#[derive(Deserialize)]
struct BraveSearchResponse {
    web: Option<BraveWebResults>,
}

#[derive(Deserialize)]
struct BraveWebResults {
    results: Vec<BraveSearchResult>,
}

#[derive(Deserialize)]
struct BraveSearchResult {
    title: String,
    url: String,
    description: String,
}
