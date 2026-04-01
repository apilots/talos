use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::apply_patch::{run, ApplyPatchRequest};
use crate::{Tool, ToolContext, ToolOutput};

pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply a verified patch body using the Codex apply_patch format."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["input"],
            "properties": {
                "input": { "type": "string" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: ApplyPatchRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid apply_patch arguments: {}", err))
        })?;
        let response = run(&request, &context).await?;
        Ok(ToolOutput {
            success: true,
            output: response.output,
            error: None,
        })
    }
}
