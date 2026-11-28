use talos_agent_api::{trajectory::SessionHistorySupplement, SessionHistoryTurn};

use crate::{AgentFuture, AgentResult};

/// Selects one bounded canonical Session history window for internal consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionHistoryWindow {
    Latest,
    OldestAfter {
        /// Exclusive Worker fact position, including message creation positions
        /// inside an already completed Turn when resuming a partial window.
        completed_event_seq: i64,
    },
}

/// Parameters for reading canonical completed Turns without transport cursors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHistoryReadParams {
    pub session_id: String,
    pub window: SessionHistoryWindow,
    pub limit: usize,
    /// Maximum new message facts per Turn; bounded companion context is additional.
    pub message_limit: usize,
}

/// One canonical Turn and the contiguous message coverage consumed by this read.
/// The terminal identity and sequence remain unchanged when a Turn spans windows.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionHistoryReadTurn {
    pub turn: SessionHistoryTurn,
    pub covered_until_event_seq: i64,
    /// Coverage before this window and each included canonical message creation fact.
    pub read_after_event_seq: i64,
    pub message_event_sequences: Vec<(String, i64)>,
}

/// Existing canonical history capability injected into Agent implementations.
///
/// Implementations must return completed Turns in chronological order and must
/// bound every read by both limits and an implementation-owned byte budget.
/// `OldestAfter` resumes on canonical message creation facts within a completed Turn;
/// coverage reaches the terminal fact only after its final message is included.
/// Consumers must further restrict coverage when their projection drops a prefix.
/// Companion input/tool-call context may be repeated without advancing coverage.
/// This port never exposes Store records, Timeline
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
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryReadTurn>>>;
}
