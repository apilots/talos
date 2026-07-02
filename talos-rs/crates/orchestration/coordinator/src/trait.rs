use async_trait::async_trait;
use std::sync::Arc;
use talos_agent::AgentHarness;
use talos_error::TalosResult;
use talos_types::AgentDispatchRuntime;

use crate::SessionRuntime;

/// Coordinator facade for Agent orchestration and session runtime operations.
///
/// Implementations own Agent registration, routing, and dispatch. They should
/// depend on AgentHarness abstractions instead of concrete runtime backends or
/// Service store records.
#[async_trait]
pub trait Coordinator: AgentDispatchRuntime + SessionRuntime + Send + Sync {
    /// Register an AgentHarness under a stable Coordinator-visible Agent id.
    async fn register_agent(&self, agent_id: &str, agent: Arc<dyn AgentHarness>)
        -> TalosResult<()>;
}
