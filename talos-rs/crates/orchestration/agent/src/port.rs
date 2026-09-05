use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use talos_agent_api::AgentInterruptOutcome;

use crate::{
    AgentAdmission, AgentContext, AgentInputParams, AgentInterruptParams, AgentResult,
    AgentRunExecution, AgentRunParams, AgentSessionContext,
};

/// Object-safe future used only by the outer Agent Harness port.
pub type AgentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Provider-neutral execution host resolved and invoked by Coordinator.
///
/// Implementations own live admission, event settlement, input delivery, and
/// bounded Provider lifecycle. They must not own product persistence, queues,
/// complete Session aggregates, transport DTOs, or Provider-native APIs.
pub trait AgentHarness: Send + Sync {
    /// Admit one fresh or continued logical Turn.
    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>>;

    /// Admit one typed User message for the exact live invocation.
    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>>;

    /// Interrupt one exact live invocation and settle its resources.
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<AgentInterruptOutcome>>;

    /// Admit one explicit context compaction through canonical event delivery.
    fn compact_session(
        &self,
        context: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>>;

    /// Delete reconstructable Provider-owned state for one idle Session.
    fn clear_session(&self, params: AgentSessionContext) -> AgentFuture<'_, AgentResult<()>>;

    /// Release Provider-owned live resources for one idle Session.
    fn close_session(&self, params: AgentSessionContext) -> AgentFuture<'_, AgentResult<()>>;

    /// Stop admission and release all Harness/Provider process resources.
    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>>;
}
