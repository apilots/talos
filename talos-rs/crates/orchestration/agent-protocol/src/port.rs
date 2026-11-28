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
    /// Read already committed execution facts. Every transport preserves the
    /// same authorization, generation, byte and record limits.
    fn read_session_events(
        &self,
        _params: crate::AgentSessionEventsParams,
    ) -> AgentFuture<'_, AgentResult<crate::AgentSessionEventsResult>> {
        Box::pin(async {
            Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Agent execution replay is unavailable.",
            ))
        })
    }

    /// Read sealed trajectory content at its execution owner, never a Server prompt callback.
    fn read_execution_payload(
        &self,
        _params: crate::AgentExecutionPayloadParams,
    ) -> AgentFuture<'_, AgentResult<crate::AgentExecutionPayloadResult>> {
        Box::pin(async {
            Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Worker execution payload reads are unavailable.",
            ))
        })
    }

    /// Read the Worker's causal boundary; no history or execution claim is returned.
    fn read_session_execution_state(
        &self,
        _session: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<Option<crate::SessionExecutionState>>> {
        Box::pin(async { Ok(None) })
    }
    /// Execution-owner-only lookup of an exact committed start request.
    /// Returns None for a new command and never reserves or starts Provider work.
    /// Carriers must not use start as a replay probe. Unsupported owners fail explicitly;
    /// this local journal hook is not an additional network operation.
    fn replay_admitted_run(
        &self,
        _params: AgentRunParams,
    ) -> AgentFuture<'_, AgentResult<Option<AgentRunExecution>>> {
        Box::pin(async {
            Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Worker admission replay is unavailable.",
            ))
        })
    }

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
    fn clear_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>>;

    /// Release Provider-owned live resources for one idle Session.
    /// Permanently retire a deleted Session after confirmed idle Provider cleanup.
    /// The Worker retains a local tombstone and rejects future admission for this identity.
    fn retire_session(
        &self,
        _session: AgentSessionContext,
        _expected: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async {
            Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Worker Session retirement is unavailable.",
            ))
        })
    }

    fn close_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>>;

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

    /// Propose a title from one successful local Turn, retaining its result and actual usage.
    /// No model routing, prompts or product history cross this boundary. A missing result
    /// means no title input or retained result exists; product metadata remains caller-owned.
    fn generate_session_title(
        &self,
        _params: crate::AgentSessionTitleParams,
    ) -> AgentFuture<'_, AgentResult<Option<crate::AgentSessionTitleResult>>> {
        Box::pin(async {
            Ok(Some(crate::AgentSessionTitleResult {
                outcome: crate::AgentSessionTitleOutcome::Failed {
                    error: talos_error::TalosError::new(
                        talos_error::ErrorCode::ServiceUnavailable,
                        "Session title generation is unavailable on this Worker.",
                    )
                    .to_api_error(),
                },
                model_usage: None,
            }))
        })
    }

    /// Form one idle Session's memory using execution-node policy, date and files.
    /// The caller excludes Turn admission and idempotently settles returned usage before
    /// advancing its own consumption cursor. The Worker retains execution facts.
    fn form_dream_memory(
        &self,
        _params: crate::DreamMemoryParams,
    ) -> AgentFuture<'_, AgentResult<crate::DreamMemoryResult>> {
        memory_unavailable()
    }

    /// Local Worker-host phase: stop admission and settle active execution while
    /// retaining Provider resources and authorized committed-fact reads.
    /// Remote clients do not own this phase and expose no transport operation for it.
    fn stop_execution(&self, _timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async {
            Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Local Worker execution stopping is unavailable.",
            ))
        })
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
