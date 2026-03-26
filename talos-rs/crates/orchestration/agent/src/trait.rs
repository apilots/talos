use crate::AgentResponse;
use async_trait::async_trait;
use std::any::Any;
use talos_error::TalosResult;

#[async_trait]
pub trait Agent: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    async fn execute_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> TalosResult<AgentResponse>;
}
