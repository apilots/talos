use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::fs_diff::{build_file_diff_item, FileDiffInput};
use crate::runtimes::fs_edit::{run, EditFileRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

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

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        parse_edit_request(input)?;
        Ok(())
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request = parse_edit_request(&input)?;
        let result = run(&request, &context).await?;
        let structured = serde_json::to_value(&result).map_err(|err| {
            talos_error::TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("serialize edit response: {}", err),
            )
        })?;
        let diff = build_file_diff_item(FileDiffInput {
            path: &result.path,
            old_path: None,
            original: &result.original_file,
            updated: &result.updated_file,
        });
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            vec![diff],
        ))
    }
}

fn parse_edit_request(input: &Value) -> TalosResult<EditFileRequest> {
    let request: EditFileRequest = serde_json::from_value(input.clone()).map_err(|err| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid edit arguments: {err}"),
        )
    })?;

    if request.path.is_none() && request.file_path.is_none() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "path is required".to_string(),
        ));
    }
    let old_string = request.find.as_deref().or(request.old_string.as_deref());
    if old_string.is_none() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "find/old_string is required".to_string(),
        ));
    }
    let new_string = request.replace.as_deref().or(request.new_string.as_deref());
    if new_string.is_none() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "replace/new_string is required".to_string(),
        ));
    }
    if old_string == new_string {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "old_string and new_string must differ".to_string(),
        ));
    }

    Ok(request)
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
