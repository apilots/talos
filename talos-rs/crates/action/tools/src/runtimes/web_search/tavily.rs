use async_trait::async_trait;
use std::time::Duration;
use talos_config::{RuntimeEnv, WebSearchProviderRuntimeConfig};
use talos_error::{TalosError, TalosResult};
use tavily::{SearchRequest, Tavily};

use super::{
    provider_result_limit, resolve_api_key, SearchCandidate, SearchProvider, WebSearchRequest,
};

pub(super) struct TavilySearchProvider {
    api_key: String,
    client: Tavily,
}

impl TavilySearchProvider {
    pub(super) fn new(
        config: WebSearchProviderRuntimeConfig,
        runtime_env: &RuntimeEnv,
    ) -> TalosResult<Self> {
        let api_key = resolve_api_key(&config, runtime_env, "tavily")?;
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("https://api.tavily.com");
        let client = Tavily::builder(&api_key)
            .base_url(base_url)
            .timeout(Duration::from_millis(config.timeout_ms.max(1)))
            .build()
            .map_err(|err| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("build Tavily search client: {err}"),
                )
            })?;
        Ok(Self { api_key, client })
    }
}

#[async_trait]
impl SearchProvider for TavilySearchProvider {
    async fn search(&self, request: &WebSearchRequest) -> TalosResult<Vec<SearchCandidate>> {
        let mut search = SearchRequest::new(&self.api_key, request.query.trim())
            .max_results(provider_result_limit(request) as i32)
            .include_answer(false)
            .include_raw_content(false);
        if !request.allowed_domains.is_empty() {
            search = search.include_domains(request.allowed_domains.clone());
        }
        if !request.blocked_domains.is_empty() {
            search = search.exclude_domains(request.blocked_domains.clone());
        }

        let response = self.client.call(&search).await.map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("Tavily search failed: {err}"),
            )
        })?;
        Ok(response
            .results
            .into_iter()
            .map(|result| SearchCandidate {
                title: result.title,
                url: result.url,
                snippet: result.content,
            })
            .collect())
    }

    fn supports_recency(&self) -> bool {
        false
    }
}
