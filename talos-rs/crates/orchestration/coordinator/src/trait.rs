use std::time::Duration;

use async_trait::async_trait;
use talos_agent::{
    AgentAdmission, AgentContext, AgentInputParams, AgentInterruptParams, AgentRunExecution,
    AgentRunParams, AgentSessionContext,
};
use talos_agent_api::AgentInterruptOutcome;
use talos_error::TalosResult;

use crate::AgentSelectionParams;

/// Single Service-to-Agent orchestration port.
///
/// Implementations resolve one immutable Agent binding and forward the same
/// typed objects to its Harness. They must not construct product Context,
/// persist events, interpret concrete Provider configuration, or expose transport DTOs.
#[async_trait]
pub trait AgentCoordinator: Send + Sync {
    /// Resolve the Agent selected for new Session creation.
    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String>;

    /// Validate that one Agent binding is registered.
    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()>;

    /// Forward one fresh or continuation Turn admission to its bound Harness.
    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution>;

    /// Forward live User input admission to its bound Harness.
    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission>;

    /// Interrupt one exact live invocation.
    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome>;

    /// Run one explicit Provider-owned Session compaction.
    async fn compact_session(&self, context: AgentContext) -> TalosResult<AgentRunExecution>;

    /// Clear reconstructable Provider-owned Session state.
    async fn clear_session(&self, params: AgentSessionContext) -> TalosResult<()>;

    /// Release Provider-owned live Session resources.
    async fn close_session(&self, params: AgentSessionContext) -> TalosResult<()>;

    /// Stop new admission and release all registered Harness resources.
    async fn shutdown(&self, timeout: Duration) -> TalosResult<()>;
}
