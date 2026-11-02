use talos_agent_api::{trajectory::SessionHistorySupplement, SessionHistoryTurn};

use crate::{AgentFuture, AgentResult};

/// Selects one bounded canonical Session history window for internal consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionHistoryWindow {
    Latest,
    OldestAfter { completed_event_seq: i64 },
}

/// Parameters for reading canonical completed Turns without transport cursors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHistoryReadParams {
    pub session_id: String,
    pub window: SessionHistoryWindow,
    pub limit: usize,
}

/// Existing canonical history capability injected into Agent implementations.
///
/// Implementations must return completed Turns in chronological order and must
/// bound every read by `limit`. This port never exposes Store records, Timeline
/// envelopes, Provider transcripts, or transport cursors. Optional private execution
/// supplementation uses this same history owner without changing canonical JSON.
pub trait SessionHistoryReader: Send + Sync {
    /// Actual node identity when this history capability is invocation-bound.
    fn source_node_id(&self) -> Option<String> {
        None
    }

    /// Optional execution facts on the existing Turn history owner. Implementations
    /// must reject late writes after terminal history and retain prior wait segments.
    fn append_supplement(
        &self,
        _supplement: SessionHistorySupplement,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async {
            Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Execution history supplementation is unavailable.",
            ))
        })
    }
    fn read(
        &self,
        params: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryTurn>>>;
}
