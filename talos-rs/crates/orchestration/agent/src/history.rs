use talos_agent_api::SessionHistoryTurn;

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

/// Read-only canonical history capability injected into Agent implementations.
///
/// Implementations must return completed Turns in chronological order and must
/// bound every read by `limit`. This port never exposes Store records, Timeline
/// envelopes, Provider transcripts, or transport cursors.
pub trait SessionHistoryReader: Send + Sync {
    fn read(
        &self,
        params: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryTurn>>>;
}
