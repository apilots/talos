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
