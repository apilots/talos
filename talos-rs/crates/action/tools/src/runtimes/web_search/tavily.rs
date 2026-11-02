use async_trait::async_trait;
use std::time::Duration;
use talos_config::{RuntimeEnv, WebSearchProviderRuntimeConfig};
use talos_error::{TalosError, TalosResult};
use tavily::{SearchRequest, Tavily, TavilyError};

use super::{
    provider_result_limit, resolve_api_key, SearchCandidate, SearchProvider, WebSearchRequest,
};

pub(super) struct TavilySearchProvider {
    api_key: String,
    client: Tavily,
    timeout: Duration,
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
        let timeout = Duration::from_millis(config.timeout_ms.max(1));
        let client = Tavily::builder(&api_key)
            .base_url(base_url)
            .timeout(timeout)
            .max_retries(0)
            .build()
            .map_err(|err| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("build Tavily search client: {err}"),
                )
            })?;
        Ok(Self {
            api_key,
            client,
            timeout,
        })
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

        let response = tokio::time::timeout(self.timeout, async {
            let mut response = self.client.call(&search).await;
            for _ in 0..3 {
                if !matches!(response, Err(TavilyError::RateLimit(_))) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                response = self.client.call(&search).await;
            }
            response
        })
        .await
        .map_err(|err| {
            TalosError::from_source(
                talos_error::ErrorCode::ToolExecutionFailed,
                "Tavily search timed out.",
                err,
            )
        })?
        .map_err(|err| {
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
