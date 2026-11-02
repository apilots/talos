use async_trait::async_trait;
use firecrawl::{SearchOptions, SearchResponse, SearchResultOrDocument, SearchSource};
use serde::Serialize;
use std::time::Duration;
use talos_config::{RuntimeEnv, WebSearchProviderRuntimeConfig};
use talos_error::{TalosError, TalosResult};

use super::{
    configured_api_key, firecrawl_time_range,
    http::{search_client, search_json},
    provider_result_limit, SearchCandidate, SearchProvider, WebSearchRequest,
};

pub(super) struct FirecrawlSearchProvider {
    client: reqwest::Client,
    url: String,
    api_key: Option<String>,
    timeout_ms: u64,
}

impl FirecrawlSearchProvider {
    pub(super) fn new(
        config: WebSearchProviderRuntimeConfig,
        runtime_env: &RuntimeEnv,
    ) -> TalosResult<Self> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("https://api.firecrawl.dev");
        let api_key = configured_api_key(&config, runtime_env);
        let client = search_client(Duration::from_millis(config.timeout_ms.max(1)))?;
        Ok(Self {
            client,
            url: format!("{}/v2/search", base_url.trim_end_matches('/')),
            api_key,
            timeout_ms: config.timeout_ms,
        })
    }
}

#[async_trait]
impl SearchProvider for FirecrawlSearchProvider {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<Vec<SearchCandidate>> {
        let mut options = SearchOptions {
            limit: Some(provider_result_limit(request) as u32),
            sources: Some(vec![SearchSource::Web]),
            tbs: firecrawl_time_range(request.recency_days).map(str::to_string),
            timeout: Some(self.timeout_ms.min(u32::MAX as u64) as u32),
            ..SearchOptions::default()
        };
        if request.allowed_domains.is_empty() {
            if !request.blocked_domains.is_empty() {
                options.exclude_domains = Some(request.blocked_domains.clone());
            }
        } else {
            options.include_domains = Some(request.allowed_domains.clone());
        }

        let body = FirecrawlSearchRequest {
            query: request.query.trim(),
            options,
        };
        let mut search = self.client.post(&self.url).json(&body);
        if let Some(api_key) = &self.api_key {
            search = search.bearer_auth(api_key);
        }
        let response: SearchResponse = tokio::time::timeout(
            Duration::from_millis(self.timeout_ms.max(1)),
            search_json(search),
        )
        .await
        .map_err(|err| {
            TalosError::from_source(
                talos_error::ErrorCode::ToolExecutionFailed,
                "Firecrawl search timed out.",
                err,
            )
        })??;
        if !response.success {
            return Err(TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "Firecrawl search returned an unsuccessful response".to_string(),
            ));
        }

        Ok(response
            .data
            .web
            .unwrap_or_default()
            .into_iter()
            .filter_map(|result| match result {
                SearchResultOrDocument::WebResult(result) => {
                    let title = result.title.unwrap_or_else(|| result.url.clone());
                    Some(SearchCandidate {
                        title,
                        url: result.url,
                        snippet: result.description.unwrap_or_default(),
                    })
                }
                SearchResultOrDocument::Document(_) => None,
            })
            .collect())
    }

    fn supports_recency(&self) -> bool {
        true
    }
}

#[derive(Serialize)]
struct FirecrawlSearchRequest<'a> {
    query: &'a str,
    #[serde(flatten)]
    options: SearchOptions,
}
