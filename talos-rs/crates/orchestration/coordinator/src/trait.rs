use async_trait::async_trait;
use std::sync::Arc;
use talos_agent::Agent;
use talos_error::TalosResult;
use talos_types::{AgentDispatchRuntime, SessionRuntime};

#[async_trait]
pub trait Coordinator: AgentDispatchRuntime + SessionRuntime + Send + Sync {
    async fn register_agent(&self, agent_id: &str, agent: Arc<dyn Agent>) -> TalosResult<()>;
}
