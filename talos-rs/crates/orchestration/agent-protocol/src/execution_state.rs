use crate::AgentSessionContext;
use talos_types::AgentTurnRef;
pub use talos_types::SessionHistoryCursor;

/// Worker-owned execution boundary, not an execution record or history aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExecutionState {
    pub history_cursor: SessionHistoryCursor,
    pub active_turn: Option<AgentTurnRef>,
    pub coordinator_id: Option<String>,
    pub handoff_ready: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionStateVisibility {
    Local,
    Shared,
}
/// Caller must have persisted the exact terminal history cursor before Local, and obtained
/// H's durable acknowledgement before Shared. Worker never reads Loro or a DB.
pub struct CommitSessionExecutionStateParams {
    pub session: AgentSessionContext,
    pub identity: Option<AgentTurnRef>,
    pub history_cursor: SessionHistoryCursor,
    pub visibility: SessionStateVisibility,
}
