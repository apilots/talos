use crate::{AgentResponse, AgentStream};
use async_trait::async_trait;
use std::any::Any;
use talos_error::TalosResult;

#[async_trait]
pub trait Agent: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    async fn handle_message(&self, content: &str) -> TalosResult<String>;
    async fn handle_message_with_model(
        &self,
        content: &str,
        model: Option<&str>,
    ) -> TalosResult<String> {
        let _ = model;
        self.handle_message(content).await
    }
    async fn stream_response(
        &self,
        content: &str,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<AgentStream>>;
    async fn execute_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> TalosResult<AgentResponse>;
}
