use async_trait::async_trait;
use std::{error::Error, time::Duration};
use talos_config::{RuntimeEnv, WebSearchProviderRuntimeConfig};
use talos_error::{TalosError, TalosResult};
use tavily::{SearchRequest, SearchResponse};

use super::{
    http::{search_client, search_json},
    provider_result_limit, resolve_api_key, SearchCandidate, SearchProvider, WebSearchRequest,
};

pub(super) struct TavilySearchProvider {
    api_key: String,
    client: reqwest::Client,
    url: String,
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
        let client = search_client(timeout)?;
        Ok(Self {
            api_key,
            client,
            url: format!("{base_url}/search"),
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
            let send = || search_json::<SearchResponse>(self.client.post(&self.url).json(&search));
            let mut response = send().await;
            for _ in 0..3 {
                let rate_limited = response
                    .as_ref()
                    .err()
                    .and_then(Error::source)
                    .and_then(|source| source.downcast_ref::<reqwest::Error>())
                    .is_some_and(|source| {
                        source.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS)
                    });
                if !rate_limited {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                response = send().await;
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
        })??;
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
