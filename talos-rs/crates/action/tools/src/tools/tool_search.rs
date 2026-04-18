use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::state::CatalogToolEntry;
use crate::{Tool, ToolContext, ToolOutput};

const DEFAULT_LIMIT: usize = 10;

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolSearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolSearchResponse {
    pub query: String,
    pub matches: Vec<CatalogToolEntry>,
}

pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }

    fn internal_name(&self) -> &str {
        "tool_search"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["tool_search"]
    }

    fn description(&self) -> &str {
        "Search the current tool catalog for deferred or not-yet-visible tools."
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
            max_inline_bytes: 16 * 1024,
            persist_large_output: false,
            preview_lines: 40,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keyword query, exact tool name, or MCP prefix."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of matches to return."
                }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: ToolSearchRequest = serde_json::from_value(input).map_err(|err| {
            TalosError::ToolError(format!("invalid tool_search arguments: {err}"))
        })?;
        let query = request.query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return Err(TalosError::ToolError(
                "tool_search query must not be empty".to_string(),
            ));
        }
        if request.limit == 0 {
            return Err(TalosError::ToolError(
                "tool_search limit must be greater than zero".to_string(),
            ));
        }

        let matches = context
            .tool_catalog_state()
            .list()
            .into_iter()
            .filter(|entry| entry.public_name != "http_request")
            .filter(|entry| matches_query(entry, &query))
            .take(request.limit)
            .collect::<Vec<_>>();
        for entry in &matches {
            context
                .deferred_tool_state()
                .activate(entry.public_name.clone());
        }
        let response = ToolSearchResponse {
            query: request.query,
            matches,
        };

        Ok(ToolOutput {
            success: true,
            output: serde_json::to_string(&response).map_err(|err| {
                TalosError::ToolError(format!("serialize tool_search response: {err}"))
            })?,
            error: None,
        })
    }
}

fn matches_query(entry: &CatalogToolEntry, query: &str) -> bool {
    let public_name = entry.public_name.to_ascii_lowercase();
    let description = entry.description.to_ascii_lowercase();
    public_name == query
        || public_name.starts_with(query)
        || public_name.contains(query)
        || description.contains(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn tool_search_uses_catalog_snapshot() {
        let session_id = format!("session-{}", Uuid::new_v4());
        let context = ToolContext::new(session_id, "user-1").with_catalog_snapshot(vec![
            CatalogToolEntry {
                public_name: "WebSearch".to_string(),
                description: "Search the web".to_string(),
                deferred: false,
                available: true,
                unavailable_reason: None,
            },
            CatalogToolEntry {
                public_name: "http_request".to_string(),
                description: "Legacy HTTP tool".to_string(),
                deferred: false,
                available: true,
                unavailable_reason: None,
            },
        ]);

        let tool = ToolSearchTool;
        let result = tool
            .execute(
                serde_json::json!({
                    "query": "web"
                }),
                context.clone(),
            )
            .await
            .expect("tool_search should succeed");

        assert!(result.output.contains("WebSearch"));
        assert!(!result.output.contains("http_request"));
        assert_eq!(
            context.deferred_tool_state().list(),
            vec!["WebSearch".to_string()]
        );
    }
}
