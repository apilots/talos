use async_trait::async_trait;
use serde_json::Value;

use crate::{Tool, ToolContext, ToolOutput};
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

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let output = input.as_str().unwrap_or(&input.to_string()).to_string();
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTestTool;
        let input = serde_json::json!("hello world");
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert_eq!(result.output, "hello world");
    }
}
