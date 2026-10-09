use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use talos_agent_api::AgentInterruptOutcome;

use crate::{
    AgentAdmission, AgentContext, AgentInputParams, AgentInterruptParams, AgentResult,
    AgentRunExecution, AgentRunParams, AgentSessionContext,
};

/// Object-safe future used only by the outer Agent Harness port.
pub type AgentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Invocation-scoped model callback. The control plane owns model selection and usage.
/// Dream formation consumes only System/User text and response content/token usage.
pub type DreamMemoryModelCall = Arc<
    dyn Fn(
            Vec<talos_llm_provider::ChatMessage>,
        ) -> AgentFuture<'static, AgentResult<talos_llm_provider::ChatResponse>>
        + Send
        + Sync,
>;

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

    /// Access the Session's execution-node memory using the existing memory contract.
    /// Hosts without Workspace memory fail explicitly; transport implementations preserve this result.
    fn read_workspace_memory(
        &self,
        _context: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        memory_unavailable()
    }

    /// Access the Session's execution-node memory using the existing memory contract.
    /// Hosts without Workspace memory fail explicitly; transport implementations preserve this result.
    fn update_workspace_memory(
        &self,
        _context: AgentSessionContext,
        _update: talos_memory::UserWorkspaceMemoryUpdate,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        memory_unavailable()
    }

    /// Form one idle Session's memory using execution-node policy, date and files.
    /// The caller must exclude concurrent Turn admission and persist model usage.
    fn form_dream_memory(
        &self,
        _context: AgentSessionContext,
        _model_call: DreamMemoryModelCall,
    ) -> AgentFuture<'_, AgentResult<Option<u64>>> {
        memory_unavailable()
    }

    /// Stop admission and release all Harness/Provider process resources.
    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>>;
}

fn memory_unavailable<T>() -> AgentFuture<'static, AgentResult<T>> {
    Box::pin(async {
        Err(talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Agent host does not provide Workspace memory",
        ))
    })
}
