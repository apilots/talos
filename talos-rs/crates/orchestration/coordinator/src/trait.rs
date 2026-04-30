use async_trait::async_trait;
use std::sync::Arc;
use talos_agent::AgentOrchestrator;
use talos_error::TalosResult;
use talos_types::{RequestHandler, SessionRuntime};

#[async_trait]
pub trait Coordinator: RequestHandler + SessionRuntime + Send + Sync {
    async fn register_agent(
        &self,
        agent_id: &str,
        agent: Arc<dyn AgentOrchestrator>,
    ) -> TalosResult<()>;
}
