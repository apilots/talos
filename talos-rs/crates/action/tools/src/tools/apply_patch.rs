use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosContentItem;

use crate::runtimes::fs_apply_patch::{run, ApplyPatchRequest};
use crate::runtimes::fs_diff::{build_file_diff_item, FileDiffInput};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "ApplyPatch"
    }

    fn internal_name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply a multi-file patch to the local filesystem.\n\nUsage:\n- The patch parameter must contain a patch starting with `*** Begin Patch` and ending with `*** End Patch`.\n- Add-file content lines must start with `+` directly after the `*** Add File:` line.\n- Do not use `---`, `+++`, or markdown code fences inside the patch text.\n- Example add-file patch:\n*** Begin Patch\n*** Add File: notes/example.txt\n+first line\n+second line\n*** End Patch\n- Supports `*** Add File:`, `*** Update File:`, `*** Delete File:`, and `*** Move to:` operations.\n- Update, delete, and move operations require the target file to have been read in the current session.\n- Use this tool for coordinated multi-file edits; use Edit for small exact replacements."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["patch"],
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Patch text using Begin Patch / End Patch markers. For add-file patches, every content line must start with '+'."
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
            preview_lines: 120,
        }
    }

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        parse_apply_patch_request(input)?;
        Ok(())
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request = parse_apply_patch_request(&input)?;
        let result = run(&request, &context).await?;
        let structured = serde_json::to_value(&result).map_err(|err| {
            TalosError::ToolError(format!("serialize apply patch response: {}", err))
        })?;
        let mut files = Vec::new();
        for change in &result.changes {
            let TalosContentItem::Diff { files: diff_files } =
                build_file_diff_item(FileDiffInput {
                    path: &change.path,
                    old_path: change.old_path.as_deref(),
                    original: &change.original_file,
                    updated: &change.updated_file,
                })
            else {
                continue;
            };
            files.extend(diff_files);
        }
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            vec![TalosContentItem::Diff { files }],
        ))
    }
}

fn parse_apply_patch_request(input: &Value) -> TalosResult<ApplyPatchRequest> {
    let request: ApplyPatchRequest = serde_json::from_value(input.clone()).map_err(|err| {
        TalosError::ValidationError(format!("invalid apply patch arguments: {err}"))
    })?;
    if request.patch.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "patch must not be empty".to_string(),
        ));
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_input_rejects_missing_patch() {
        let tool = ApplyPatchTool;
        let error = tool.validate_input(&serde_json::json!({})).unwrap_err();

        assert!(matches!(error, TalosError::ValidationError(_)));
    }

    #[tokio::test]
    async fn execute_payload_returns_diff_content_item() {
        let dir = tempfile::tempdir().expect("temp dir should create");
        let path = dir.path().join("demo.txt");
        let tool = ApplyPatchTool;

        let payload = tool
            .execute_payload(
                serde_json::json!({
                    "patch": format!(
                        "*** Begin Patch\n*** Add File: {}\n+alpha\n*** End Patch",
                        path.display()
                    )
                }),
                ToolContext::new("session", "user"),
            )
            .await
            .expect("patch should apply");

        let ToolOutputPayload::Function { content_items, .. } = payload else {
            panic!("apply patch should return function payload");
        };
        assert!(matches!(
            content_items.first(),
            Some(TalosContentItem::Diff { files }) if files.len() == 1
        ));
    }
}
