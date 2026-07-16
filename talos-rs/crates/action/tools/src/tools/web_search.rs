use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosContentItem;

use crate::runtimes::web_search::{run_with_config, WebSearchRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn internal_name(&self) -> &str {
        "web_search"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["web_search"]
    }

    fn description(&self) -> &str {
        "Search the public web for fresh information and return structured result hits."
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: true,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 48 * 1024,
            persist_large_output: false,
            preview_lines: 50,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query text."
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain allowlist."
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional domain blocklist."
                },
                "recency_days": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Requested freshness window in days. The structured response reports whether the selected provider applied it."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20,
                    "description": "Maximum number of hits to return."
                }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: WebSearchRequest = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid web_search arguments: {err}")))?;
        let default_config;
        let config = if let Some(kernel_config) = context.kernel_config() {
            &kernel_config.tools.web_search
        } else {
            default_config = talos_config::WebSearchToolConfig::default();
            &default_config
        };
        let response = run_with_config(&request, config).await?;
        let structured = serde_json::to_value(&response).map_err(|err| {
            TalosError::ToolError(format!("serialize web_search response: {err}"))
        })?;
        let mut content_items = response
            .hits
            .iter()
            .map(|hit| TalosContentItem::WebReference {
                url: hit.url.clone(),
                title: Some(hit.title.clone()),
                citation_id: Some(format!("web-{}", hit.rank)),
                snippet: Some(hit.snippet.clone()).filter(|value| !value.trim().is_empty()),
                source_domain: hit.source_domain.clone(),
                published_at: None,
                provider_rank: Some(hit.rank),
            })
            .collect::<Vec<_>>();
        if content_items.is_empty() {
            content_items.push(TalosContentItem::Text {
                text: "No search results matched the requested filters.".to_string(),
            });
        }
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            content_items,
        ))
    }
}

#[cfg(test)]
#[path = "web_search_tests.rs"]
mod tests;
