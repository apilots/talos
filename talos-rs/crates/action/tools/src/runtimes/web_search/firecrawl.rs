use async_trait::async_trait;
use firecrawl::{Client, SearchOptions, SearchResultOrDocument, SearchSource};
use talos_config::WebSearchProviderRuntimeConfig;
use talos_error::{TalosError, TalosResult};

use super::{
    configured_api_key, firecrawl_time_range, provider_result_limit, SearchCandidate,
    SearchProvider, WebSearchRequest,
};

pub(super) struct FirecrawlSearchProvider {
    client: Client,
    timeout_ms: u64,
}

impl FirecrawlSearchProvider {
    pub(super) fn new(config: WebSearchProviderRuntimeConfig) -> TalosResult<Self> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or("https://api.firecrawl.dev");
        let api_key = configured_api_key(&config);
        let client = Client::new_selfhosted(base_url, api_key.as_deref()).map_err(|err| {
            TalosError::InvalidConfiguration(format!("build Firecrawl search client: {err}"))
        })?;
        Ok(Self {
            client,
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

        let response = self
            .client
            .search(request.query.trim(), options)
            .await
            .map_err(|err| TalosError::ToolError(format!("Firecrawl search failed: {err}")))?;
        if !response.success {
            return Err(TalosError::ToolError(
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
