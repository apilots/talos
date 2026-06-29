use async_trait::async_trait;
use serde_json::Value;

use crate::{Tool, ToolContext, ToolOutputPayload};
use talos_error::TalosResult;

/// Test tool for echoing input
pub struct EchoTestTool;

#[async_trait]
impl Tool for EchoTestTool {
    fn name(&self) -> &str {
        "echo_test"
    }

    fn description(&self) -> &str {
        "Echoes the input back as output"
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "value": {"type": ["string", "number", "boolean", "object", "array", "null"]}
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let output = input
            .as_str()
            .map(ToString::to_string)
            .unwrap_or_else(|| input.to_string());
        Ok(ToolOutputPayload::success_text(output))
    }
}

#[cfg(test)]
#[path = "trait_impl_tests.rs"]
mod tests;
