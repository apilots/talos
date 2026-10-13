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

    /// Bind a new Session, or validate its parent's pinned execution node.
    async fn select_execution_node(
        &self,
        agent_id: &str,
        user_id: &str,
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String>;

    /// Resolve a node's environment after checking trusted actor ownership and availability.
    /// No Session is required. Omitted product targets are resolved by Service before calling.
    async fn worker_environment(
        &self,
        node_id: &str,
        user_id: &str,
    ) -> TalosResult<std::sync::Arc<dyn agent_protocol::WorkerEnvironment>> {
        let _ = (node_id, user_id);
        Err(talos_error::TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Worker environment is unavailable",
        ))
    }

    /// Resolve the Session's pinned execution port for Workspace maintenance.
    /// The caller must use trusted Session context; node ownership and availability are checked here.
    async fn session_harness(
        &self,
        context: &AgentSessionContext,
    ) -> TalosResult<std::sync::Arc<dyn talos_agent::AgentHarness>> {
        let _ = context;
        Err(talos_error::TalosError::internal(
            "Coordinator does not provide Session maintenance",
        ))
    }

    /// Forward causal state without interpreting history, transport or persistence.
    async fn read_session_execution_state(
        &self,
        _session: AgentSessionContext,
    ) -> TalosResult<Option<agent_protocol::SessionExecutionState>> {
        Ok(None)
    }
    async fn commit_session_execution_state(
        &self,
        _params: agent_protocol::CommitSessionExecutionStateParams,
    ) -> TalosResult<()> {
        Ok(())
    }

    /// Forward one fresh or continuation Turn admission to its bound Harness.
    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution>;

    /// Forward live User input admission to its bound Harness.
    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission>;

    /// Interrupt one exact live invocation.
    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome>;

    /// Run one explicit Provider-owned Session compaction.
    async fn compact_session(&self, context: AgentContext) -> TalosResult<AgentRunExecution>;

    /// Clear reconstructable Provider-owned Session state.
    async fn clear_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()>;

    /// Release Provider-owned live Session resources.
    async fn close_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()>;

    /// Release the Harness resources owned by this process. Independent nodes keep running.
    async fn shutdown(&self, timeout: Duration) -> TalosResult<()>;
}
