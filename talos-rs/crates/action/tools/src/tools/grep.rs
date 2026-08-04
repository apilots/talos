use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;
use talos_types::talos::TalosContentItem;

use crate::runtimes::fs_search::{run, GrepFilesRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn internal_name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "A powerful search tool built on ripgrep.\n\nUsage:\n- Always use Grep for search tasks instead of shell grep or rg.\n- Supports full regex syntax.\n- Filter files with glob patterns.\n- Output modes: content, files_with_matches, or count.\n- Use multiline=true for cross-line patterns.\n- After Grep narrows candidate files, switch to Read for detailed inspection."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Ripgrep pattern to search for, usually a regex or exact symbol text."
                },
                "include": {
                    "type": "string",
                    "description": "Optional file glob filter such as *.rs or src/**/*.ts to narrow searched files."
                },
                "glob": {
                    "type": "string",
                    "description": "Preferred file glob filter alias."
                },
                "type": {
                    "type": "string",
                    "description": "Ripgrep file type filter, such as rust, ts, js, py, or md."
                },
                "path": {
                    "type": "string",
                    "description": "Optional absolute path or workspace-relative path to limit the search scope."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of matches to return."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Offset into the result set."
                },
                "head_limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Alias for maximum number of matches to return, matching ripgrep-oriented usage."
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "Controls whether to return matching files, matching content, or only the total count."
                },
                "context": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Number of before and after context lines to include."
                },
                "before_context": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Number of context lines before each match."
                },
                "after_context": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Number of context lines after each match."
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline regex mode."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Perform case-insensitive matching."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum time to allow ripgrep to run before returning partial output."
                }
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
            max_inline_bytes: 64 * 1024,
            persist_large_output: false,
            preview_lines: 100,
        }
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: GrepFilesRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("invalid grep arguments: {}", err),
            )
        })?;
        let response = run(&request, &context).await?;
        let structured = serde_json::to_value(&response).map_err(|err| {
            talos_error::TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("serialize Grep response: {}", err),
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
#[path = "grep_tests.rs"]
mod tests;
