use async_trait::async_trait;
use std::sync::Arc;
use talos_agent::AgentHarness;
use talos_error::TalosResult;
use talos_types::AgentDispatchRuntime;

use crate::SessionRuntime;

#[async_trait]
pub trait Coordinator: AgentDispatchRuntime + SessionRuntime + Send + Sync {
    async fn register_agent(&self, agent_id: &str, agent: Arc<dyn AgentHarness>)
        -> TalosResult<()>;
}
