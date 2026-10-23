use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosContentItem;

use crate::runtimes::fs_read::{requested_file_system_access, run, ReadFileRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};
use talos_tools_sandbox::{RequestedFileSystemAccess, ToolSandboxOperationKind};

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
        "Read a file from the local filesystem.\n\nUsage:\n- The file_path parameter may be absolute or workspace-relative.\n- By default, read from the beginning of the file; use offset and limit when you only need a specific section.\n- Line offsets start at 1. Large files support bounded line ranges; use byte_offset/max_bytes for long single lines or the returned nextByteOffset cursor. Byte ranges cannot be combined with line offset/limit.\n- This tool reads files only, not directories. If you need path expansion or directory shape, use Glob first.\n- This tool can read images, PDFs, and Jupyter notebooks when supported.\n- Re-reading an unchanged file with the same arguments is low value; only do it when you need different line ranges or changed state."
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
                "byte_offset": { "type": "integer", "minimum": 0, "description": "Raw byte position; exclusive with line offset/limit." },
                "max_bytes": { "type": "integer", "minimum": 4, "description": "Maximum bytes of text to return; exclusive with line offset/limit." },
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

    fn sandbox_operation(&self) -> Option<ToolSandboxOperationKind> {
        Some(ToolSandboxOperationKind::FileSystem)
    }

    fn requested_file_system_access(
        &self,
        input: &Value,
        context: &ToolContext,
    ) -> TalosResult<Vec<RequestedFileSystemAccess>> {
        requested_file_system_access(&parse_read_request(input)?, context)
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
        let result = run(&request, &context).await?;
        let structured = serde_json::to_value(&result).map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("serialize read response: {}", err),
            )
        })?;
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            vec![TalosContentItem::FileRead {
                path: result.path,
                offset: result.offset,
                limit: result.limit,
                total_lines: result.total_lines,
                next_byte_offset: result.next_byte_offset,
                content: result.content,
                truncated: result.truncated,
            }],
        ))
    }
}

fn parse_read_request(input: &Value) -> TalosResult<ReadFileRequest> {
    if (input.get("byte_offset").is_some() || input.get("max_bytes").is_some())
        && (input.get("offset").is_some() || input.get("limit").is_some())
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Byte ranges and line ranges cannot be combined.",
        ));
    }
    let request: ReadFileRequest = serde_json::from_value(input.clone()).map_err(|err| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid read arguments: {err}"),
        )
    })?;

    if request.path.is_none() && request.file_path.is_none() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "path is required".to_string(),
        ));
    }
    if request.offset == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "offset must be a 1-indexed line number".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "limit must be greater than zero".to_string(),
        ));
    }

    Ok(request)
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
