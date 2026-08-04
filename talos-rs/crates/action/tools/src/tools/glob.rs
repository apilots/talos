use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosContentItem;

use crate::runtimes::fs_glob::{run, GlobFilesRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn internal_name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files by glob pattern within the workspace.\n\nUsage:\n- Use Glob for path discovery instead of shell find.\n- Results are sorted and paginated.\n- Default noisy directories such as .git, node_modules, target, dist, and build are skipped."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string" },
                "path": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1 },
                "offset": { "type": "integer", "minimum": 0 }
            }
        }))
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
            max_inline_bytes: 32 * 1024,
            persist_large_output: false,
            preview_lines: 100,
        }
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: GlobFilesRequest = serde_json::from_value(input).map_err(|err| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("invalid Glob arguments: {}", err),
            )
        })?;
        let response = run(&request, &context).await?;
        let structured = serde_json::to_value(&response).map_err(|err| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("serialize Glob response: {}", err),
            )
        })?;
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            vec![TalosContentItem::SearchResults {
                query: Some(response.query),
                matches: response.matches,
                offset: response.offset,
                limit: response.limit,
                total_matches: Some(response.total_matches),
                more_available: response.more_available,
                partial: response.partial,
            }],
        ))
    }
}

#[cfg(test)]
#[path = "glob_tests.rs"]
mod tests;
