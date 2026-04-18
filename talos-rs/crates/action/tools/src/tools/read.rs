use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::fs_read::{run, ReadFileRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

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

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        parse_read_request(input)?;
        Ok(())
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request = parse_read_request(&input)?;
        let output = run(&request, &context).await?;
        Ok(ToolOutputPayload::Function {
            content: output.clone(),
            success: true,
            structured_content: Some(serde_json::json!({ "result": output })),
        })
    }
}

fn parse_read_request(input: &Value) -> TalosResult<ReadFileRequest> {
    let request: ReadFileRequest = serde_json::from_value(input.clone())
        .map_err(|err| TalosError::ValidationError(format!("invalid read arguments: {err}")))?;

    if request.path.is_none() && request.file_path.is_none() {
        return Err(TalosError::ValidationError("path is required".to_string()));
    }
    if request.offset == 0 {
        return Err(TalosError::ValidationError(
            "offset must be a 1-indexed line number".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(TalosError::ValidationError(
            "limit must be greater than zero".to_string(),
        ));
    }

    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_input_rejects_missing_path() {
        let tool = ReadTool;
        let error = tool
            .validate_input(&serde_json::json!({"offset": 1, "limit": 10}))
            .unwrap_err();

        assert!(matches!(error, TalosError::ValidationError(_)));
    }
}
