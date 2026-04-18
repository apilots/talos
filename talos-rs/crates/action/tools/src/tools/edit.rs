use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::fs_edit::{run, EditFileRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutput};

/// Tool for editing file content with exact replacement semantics.
pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn internal_name(&self) -> &str {
        "edit"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["edit"]
    }

    fn description(&self) -> &str {
        "Perform exact string replacements in files.\n\nUsage:\n- You must use Read at least once before editing the file.\n- old_string must match the target text exactly, including whitespace and indentation.\n- The edit fails if old_string is not unique unless replace_all=true.\n- Prefer editing existing files instead of rewriting them wholesale."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path", "old_string", "new_string"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path or workspace-relative path to the file to edit."
                },
                "file_path": {
                    "type": "string",
                    "description": "Legacy alias for path."
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text."
                },
                "find": {
                    "type": "string",
                    "description": "Legacy alias for old_string."
                },
                "replace": {
                    "type": "string",
                    "description": "Legacy alias for new_string."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences instead of requiring a single exact match."
                }
            }
        }))
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: true,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 32 * 1024,
            persist_large_output: false,
            preview_lines: 80,
        }
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: EditFileRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid edit arguments: {}", err))
        })?;
        let result = run(&request, &context).await?;
        Ok(ToolOutput {
            success: true,
            output: serde_json::to_string(&result).map_err(|err| {
                talos_error::TalosError::ToolError(format!("serialize edit response: {}", err))
            })?,
            error: None,
        })
    }
}
