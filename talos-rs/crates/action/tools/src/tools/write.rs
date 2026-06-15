use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::fs_diff::{build_file_diff_item, FileDiffInput};
use crate::runtimes::fs_write::{run, WriteFileRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

/// Tool for writing content to a file
pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    fn internal_name(&self) -> &str {
        "write"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["write"]
    }

    fn description(&self) -> &str {
        "Write a file to the local filesystem.\n\nUsage:\n- This tool overwrites the target file if it already exists.\n- If this is an existing file, you must have used Read first in the current session.\n- Prefer Edit for modifying existing files; use Write for new files or complete rewrites.\n- Do not create documentation or README files unless the user explicitly requests them."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path or workspace-relative path to the file to create or overwrite."
                },
                "file_path": {
                    "type": "string",
                    "description": "Legacy alias for path."
                },
                "content": {
                    "type": "string",
                    "description": "Full file contents to write. This replaces the entire file."
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
            preview_lines: 60,
        }
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: WriteFileRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid write arguments: {}", err))
        })?;
        let result = run(&request, &context).await?;
        let structured = serde_json::to_value(&result).map_err(|err| {
            talos_error::TalosError::ToolError(format!("serialize write response: {}", err))
        })?;
        let diff = build_file_diff_item(FileDiffInput {
            path: &result.path,
            old_path: None,
            original: result.original_file.as_deref().unwrap_or(""),
            updated: &request.content,
        });
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            vec![diff],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_types::talos::TalosContentItem;

    #[tokio::test]
    async fn execute_payload_returns_diff_content_item() {
        let dir = tempfile::tempdir().expect("temp dir should create");
        let path = dir.path().join("demo.txt");
        let tool = WriteTool;

        let payload = tool
            .execute_payload(
                serde_json::json!({"path": path, "content": "alpha\n"}),
                ToolContext::new("session", "user"),
            )
            .await
            .expect("write should succeed");

        let ToolOutputPayload::Function { content_items, .. } = payload else {
            panic!("write should return function payload");
        };
        assert!(matches!(
            content_items.first(),
            Some(TalosContentItem::Diff { files }) if files.len() == 1
        ));
    }
}
