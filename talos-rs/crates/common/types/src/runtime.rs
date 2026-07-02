use crate::agent_dispatch;
use crate::talos;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Return a millisecond timestamp.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Parameters for cancelling the active turn through the internal runtime boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelActiveTurnParams {
    /// Prepared session used to resolve the owning agent.
    pub session: crate::session::TalosSession,
    /// Active turn identity from TalosService.
    pub turn_id: String,
}

/// Runtime boundary used by TalosService to dispatch work to AgentCoordinator.
///
/// Public transports must call Gateway. This trait is only for the internal
/// TalosService -> AgentCoordinator dispatch boundary.
#[async_trait]
pub trait AgentDispatchRuntime: Send + Sync {
    /// Dispatch a prepared agent request and return a response.
    async fn handle(
        &self,
        request: agent_dispatch::AgentDispatchParams,
    ) -> crate::TalosResult<agent_dispatch::AgentDispatchResult>;

    /// Return the internal domain timeline stream before TalosService persistence.
    ///
    /// Public transports must expose `TimelineEventEnvelope` after TalosService records
    /// these domain events and attaches recovery cursor metadata.
    async fn handle_stream(
        &self,
        request: agent_dispatch::AgentDispatchParams,
    ) -> crate::TalosResult<tokio::sync::mpsc::Receiver<talos::AgentEvent>>;

    /// Deliver structured input to a running Agent through Coordinator.
    ///
    /// The dispatch layer only routes and normalizes the input. Product state
    /// validation and persistence stay in TalosService.
    async fn submit_input(
        &self,
        params: agent_dispatch::AgentDispatchInputParams,
    ) -> crate::TalosResult<()>;

    /// Take an optional Agent-produced title for a completed session turn.
    ///
    /// This is a best-effort metadata hook. Implementations may return None
    /// when no title update exists; it must not be used as a fallback dispatch
    /// path or as required run state.
    async fn take_generated_session_title(
        &self,
        _session_id: &str,
        _turn_id: &str,
    ) -> Option<String> {
        None
    }
}
