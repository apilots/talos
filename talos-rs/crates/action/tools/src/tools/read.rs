use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::fs_read::{run, ReadFileRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutput};

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "Read"
    }

    fn internal_name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a file from the local filesystem.\n\nUsage:\n- The file_path parameter may be absolute or workspace-relative.\n- By default, read from the beginning of the file; use offset and limit when you only need a specific section.\n- Results are returned with numbered lines starting at 1.\n- This tool reads files only, not directories. If you need path expansion or directory shape, use Glob first.\n- This tool can read images, PDFs, and Jupyter notebooks when supported.\n- Re-reading an unchanged file with the same arguments is low value; only do it when you need different line ranges or changed state."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path or workspace-relative path to the file to read."
                },
                "file_path": {
                    "type": "string",
                    "description": "Legacy alias for path."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based line number to start reading from. Omit to read from the beginning."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of lines to return. Omit to use the default read limit."
                },
                "pages": {
                    "type": "string",
                    "description": "Optional page selector reserved for PDF-oriented reads."
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
            preview_lines: 200,
        }
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: ReadFileRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid read arguments: {}", err))
        })?;
        let output = run(&request, &context).await?;
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}
