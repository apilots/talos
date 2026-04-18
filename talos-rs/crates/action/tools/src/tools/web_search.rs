use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::web_search::{run, WebSearchRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutput};

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
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of hits to return."
                }
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let request: WebSearchRequest = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid web_search arguments: {err}")))?;
        let response = run(&request).await?;
        Ok(ToolOutput {
            success: true,
            output: serde_json::to_string(&response).map_err(|err| {
                TalosError::ToolError(format!("serialize web_search response: {err}"))
            })?,
            error: None,
        })
    }
}
