use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use talos_error::TalosResult;

use crate::context::ToolContext;

/// Tool execution output
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

/// Base trait for all tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name
    fn name(&self) -> &str;

    /// Tool description
    fn description(&self) -> &str;

    /// Tool input JSON schema.
    fn parameters_schema(&self) -> Option<Value> {
        None
    }

    /// Execute the tool with given input
    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput>;
}

pub fn decode_tool_input<T>(input: Value, tool_name: &str) -> TalosResult<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(input).map_err(|err| {
        talos_error::TalosError::ToolError(format!("invalid {} arguments: {}", tool_name, err))
    })
}
