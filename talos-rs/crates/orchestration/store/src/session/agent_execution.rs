use async_trait::async_trait;
use talos_agent_api::{ActiveTurnState, AgentEventPayload, AgentTurnRef};
use talos_error::{TalosError, TalosResult};
pub use talos_types::IdempotentCommand;
use talos_types::TurnCommandStatus;

mod cancellation;
pub(crate) mod idempotency;
pub(crate) mod notice;
pub(crate) mod record;
pub(crate) mod settlement;
mod sqlite;
mod supplement;

/// Store metadata for either execution or product facts; Service owns wire IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEventRecord {
    pub event_seq: i64,
    pub persisted_at: i64,
    pub event: talos_agent_api::TimelineEventPayload,
}

/// Private normalized persistence record for one canonical Agent event.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentEventRecord {
    pub event_seq: i64,
    pub identity: AgentTurnRef,
    pub sequence: u64,
    pub occurred_at: i64,
    pub persisted_at: i64,
    pub payload: AgentEventPayload,
}

/// Private nonterminal index for one logical Agent Turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentActiveTurnRecord {
    pub identity: AgentTurnRef,
    pub initial_message_id: Option<String>,
    pub state: ActiveTurnState,
    /// A product cancellation request is pending independently of observed execution state.
    pub cancellation_requested: bool,
    pub last_event_seq: i64,
    /// Public action identity derived from the committed waiting event.
    pub current_action_id: Option<String>,
    pub updated_at: i64,
}

/// Result of atomically admitting cancellation for one exact active Turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCancellationAdmission {
    pub active_turn: AgentActiveTurnRecord,
    pub request_id: String,
}

/// Stable identities allocated before one idempotent command reaches the Agent runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandReservation {
    pub session_id: String,
    pub command: IdempotentCommand,
    pub idempotency_key: String,
    pub request_hash: String,
    pub request_id: String,
    pub turn_id: String,
    pub message_id: String,
    pub lease_id: String,
    pub now: i64,
    pub lease_expires_at: i64,
    pub record_expires_at: Option<i64>,
}

/// Exact lease that a named Store transaction must compare before committing a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLease {
    /// True only when the Store transferred an expired original reservation.
    pub reclaimed: bool,
    /// Original reservation time; stable across lease reclaims, never sent to Worker.
    pub created_at: i64,
    pub session_id: String,
    pub command: IdempotentCommand,
    pub idempotency_key: String,
    pub request_id: String,
    pub turn_id: String,
    pub message_id: String,
    pub lease_id: String,
}

/// Private normalized persistence record for one idempotent product command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRecord {
    pub session_id: String,
    pub command: IdempotentCommand,
    pub idempotency_key: String,
    pub request_hash: String,
    pub request_id: String,
    pub turn_id: String,
    pub message_id: Option<String>,
    pub outcome: Option<TurnCommandStatus>,
    pub committed_event_seq: Option<i64>,
    pub lease_expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub record_expires_at: Option<i64>,
}

/// Result of reserving one exact idempotent command identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyReservation {
    Reserved(CommandLease),
    Completed(IdempotencyRecord),
    InProgress,
}

/// Product projections and management state for committed Worker execution facts.
#[async_trait]
pub trait AgentExecutionStore: Send + Sync {
    async fn get_agent_event_by_sequence(
        &self,
        _identity: &AgentTurnRef,
        _sequence: u64,
    ) -> TalosResult<Option<AgentEventRecord>> {
        Err(talos_error::TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Execution event lookup is unavailable.",
        ))
    }

    /// Product materialization position; product notices never advance this cursor.
    async fn worker_event_cursor(
        &self,
        _session_id: &str,
    ) -> TalosResult<Option<agent_protocol::AgentSessionEventCursor>> {
        Err(TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Worker event materialization is unavailable",
        ))
    }

    /// Atomically materialize committed Worker facts and their contiguous source position.
    async fn project_worker_events(
        &self,
        _params: super::WorkerProjectionParams,
    ) -> TalosResult<super::WorkerProjectionCommit> {
        Err(TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Worker event materialization is unavailable",
        ))
    }

    async fn pending_worker_settlements(
        &self,
        _session_id: &str,
    ) -> TalosResult<Vec<super::WorkerSettlementRecord>> {
        Err(TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Worker settlement materialization is unavailable",
        ))
    }

    async fn complete_worker_settlement(
        &self,
        _cursor: &agent_protocol::AgentSessionEventCursor,
    ) -> TalosResult<()> {
        Err(TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Worker settlement materialization is unavailable",
        ))
    }

    /// Atomically commit a terminal product notice and acknowledge its source receipt.
    /// A repeated receipt returns None without adding another Timeline event.
    async fn complete_worker_settlement_with_notice(
        &self,
        _cursor: &agent_protocol::AgentSessionEventCursor,
        _notice: talos_agent_api::TimelineSystemNotice,
    ) -> TalosResult<Option<TimelineEventRecord>> {
        Err(TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Worker settlement notices are unavailable",
        ))
    }

    /// Observe the Session reset generation around multi-query canonical history reads.
    /// Stores without destructive resets may leave this unsupported. Persistent
    /// implementations must advance it atomically with removal of canonical facts.
    async fn history_generation(&self, _session_id: &str) -> TalosResult<Option<u64>> {
        Ok(None)
    }

    /// Read exact original Input metadata for at most 1,000 Turn identities.
    /// No rows means those Turns have no automation binding; never infer from prompts.
    async fn read_automation_input_origins(
        &self,
        identities: &[AgentTurnRef],
    ) -> TalosResult<Vec<(AgentTurnRef, talos_types::AutomationInputOrigin)>>;

    /// Whether this existing execution-history owner supports private supplements.
    fn supports_history_supplements(&self) -> bool {
        false
    }

    /// Read a terminal Turn's private facts; None means its column is NULL.
    async fn read_terminal_history_supplement(
        &self,
        _identity: &AgentTurnRef,
    ) -> TalosResult<Option<talos_agent_api::trajectory::SessionHistorySupplement>> {
        Err(TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Execution history supplementation is unavailable.",
        ))
    }

    /// Read one frozen payload without loading other Turn bodies.
    async fn read_execution_payload(
        &self,
        _identity: &AgentTurnRef,
        _payload_id: &str,
    ) -> TalosResult<Option<String>> {
        Err(TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "Execution history supplementation is unavailable.",
        ))
    }

    /// Append a product notice for an existing Turn without mutating its lifecycle.
    async fn append_timeline_notice(
        &self,
        notice: talos_agent_api::TimelineSystemNotice,
    ) -> TalosResult<TimelineEventRecord>;

    /// Read both kinds of Timeline facts using the shared bounded replay cursor.
    async fn list_timeline_events_after(
        &self,
        session_id: &str,
        after_event_seq: i64,
        limit: usize,
    ) -> TalosResult<Vec<TimelineEventRecord>>;

    /// Resolve either kind of fact for cursor ownership checks.
    async fn get_timeline_event(&self, event_seq: i64) -> TalosResult<Option<TimelineEventRecord>>;

    /// Capture the global Timeline high water, including product notices.
    async fn get_latest_timeline_event_seq(&self, session_id: &str) -> TalosResult<Option<i64>>;

    /// Read notices for at most 1,000 exact Turn identities at one Timeline high water.
    /// Results are event-ordered, with an explicit failure above 1,024 notices per Turn.
    async fn list_turn_notices(
        &self,
        identities: &[AgentTurnRef],
        through_event_seq: i64,
    ) -> TalosResult<Vec<TimelineEventRecord>>;

    /// Reserve, replay, repair, or reclaim one idempotent product command.
    async fn reserve_command(
        &self,
        reservation: CommandReservation,
    ) -> TalosResult<IdempotencyReservation>;

    /// Release an uncommitted lease after a pre-admission failure.
    async fn release_command(&self, lease: CommandLease) -> TalosResult<()>;

    /// Record one cancellation request for an exact observed active Turn.
    async fn begin_cancellation(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<AgentCancellationAdmission>;

    /// Clear only this request when runtime interruption is rejected.
    async fn restore_cancellation(
        &self,
        admission: AgentCancellationAdmission,
    ) -> TalosResult<AgentActiveTurnRecord>;

    /// Read the ordered canonical facts for one logical Turn.
    async fn list_agent_turn_events(
        &self,
        identity: &AgentTurnRef,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>>;

    /// Read canonical Session facts strictly after one Store-global sequence.
    async fn list_agent_events_after(
        &self,
        session_id: &str,
        after_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>>;

    /// Resolve one Store-global sequence for cursor ownership validation.
    async fn get_agent_event(&self, event_seq: i64) -> TalosResult<Option<AgentEventRecord>>;

    /// Read the newest canonical event sequence for one Session.
    async fn get_latest_agent_event_seq(&self, session_id: &str) -> TalosResult<Option<i64>>;

    /// Read the newest durable terminal fact for one Session.
    async fn get_latest_agent_terminal_event(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentEventRecord>>;

    /// Read the newest terminal facts before one optional Store-global sequence.
    ///
    /// Results are returned in chronological order even though selection starts
    /// from the newest fact. This is the bounded source for recent Session history.
    async fn list_latest_agent_terminal_events(
        &self,
        session_id: &str,
        before_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>>;

    /// Read terminal facts in chronological order within (after, through].
    /// An absent upper bound includes all currently committed terminal facts.
    async fn list_agent_terminal_events_after(
        &self,
        session_id: &str,
        after_event_seq: i64,
        through_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>>;

    /// Read the terminal fact for the Turn started by one exact User message.
    async fn get_agent_terminal_event_by_initial_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> TalosResult<Option<AgentEventRecord>>;

    /// Read the current nonterminal index for one Session.
    async fn get_agent_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentActiveTurnRecord>>;

    /// Shared display only. Never install this projection as a local active claim.
    async fn get_shared_active_turn(
        &self,
        _session_id: &str,
    ) -> TalosResult<Option<talos_agent_api::TimelineActiveTurn>> {
        Ok(None)
    }
}

fn execution_conflict(message: impl Into<String>) -> TalosError {
    TalosError::new(talos_error::ErrorCode::Conflict, message.into())
}

#[cfg(test)]
#[path = "agent_execution_tests.rs"]
mod tests;
