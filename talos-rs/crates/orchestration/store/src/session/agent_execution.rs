use async_trait::async_trait;
use talos_agent_api::{
    ActiveTurnState, AgentContinuationRef, AgentEvent, AgentEventPayload, AgentMessageRole,
    AgentRunMetrics, AgentTurnRef,
};
use talos_error::{TalosError, TalosResult};
pub use talos_types::IdempotentCommand;
use talos_types::TurnCommandStatus;

mod admission;
mod cancellation;
mod continuation;
mod idempotency;
pub(crate) mod notice;
pub(crate) mod record;
mod settlement;
mod sqlite;

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
    pub last_event_seq: i64,
    /// Waiting action projected from the continuation binding, not stored twice.
    pub current_action_id: Option<String>,
    pub updated_at: i64,
}

/// Result of one committed canonical event batch.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentEventCommit {
    pub events: Vec<AgentEventRecord>,
    pub active_turn: AgentActiveTurnRecord,
}

/// Result of atomically admitting cancellation for one exact active Turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCancellationAdmission {
    pub active_turn: AgentActiveTurnRecord,
    pub previous_state: ActiveTurnState,
}

/// Durable ownership state for one paused Provider continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentContinuationBindingStatus {
    Waiting,
    Claimed { claim_id: String },
}

/// Private normalized persistence record for one Provider continuation binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContinuationBindingRecord {
    pub identity: AgentTurnRef,
    pub action_id: String,
    pub provider_ref: AgentContinuationRef,
    pub expected_event_seq: i64,
    pub status: AgentContinuationBindingStatus,
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

/// Inputs atomically committed for one fresh Agent Turn.
pub struct CommitInitialAdmissionParams {
    pub events: Vec<AgentEvent>,
    pub command: Option<CommandLease>,
}

/// Inputs atomically committed for one running Turn input command.
pub struct CommitRunningInputAdmissionParams {
    pub events: Vec<AgentEvent>,
    pub expected_event_seq: i64,
    pub command: Option<CommandLease>,
}

/// Inputs atomically committed when claiming one waiting continuation.
pub struct ClaimContinuationParams {
    pub events: Vec<AgentEvent>,
    pub action_id: String,
    pub claim_id: String,
    pub expected_event_seq: i64,
    pub command: Option<CommandLease>,
}

/// Inputs atomically committed when one canonical Agent invocation becomes terminal.
pub struct CommitTerminalSettlementParams {
    pub event: AgentEvent,
    pub expected_event_seq: i64,
    pub completed_turn_metrics: Option<AgentRunMetrics>,
}

/// Narrow atomic Store commands for canonical Agent execution facts.
#[async_trait]
pub trait AgentExecutionStore: Send + Sync {
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

    /// Atomically append initial admission and create the nonterminal index.
    async fn commit_initial_admission(
        &self,
        params: CommitInitialAdmissionParams,
    ) -> TalosResult<AgentEventCommit>;

    /// Atomically append a message-free maintenance admission.
    async fn commit_maintenance_admission(
        &self,
        event: AgentEvent,
    ) -> TalosResult<AgentEventCommit>;

    /// Atomically append one progress event and CAS the active tail.
    async fn commit_progress_event(
        &self,
        event: AgentEvent,
        expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit>;

    /// Atomically append a nonempty contiguous progress batch and CAS the active tail.
    /// All events retain their original identity and sequence; capture shares this transaction.
    async fn commit_progress_events(
        &self,
        events: Vec<AgentEvent>,
        expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit>;

    /// Atomically append one accepted live-input admission and advance the tail.
    async fn commit_running_input_admission(
        &self,
        params: CommitRunningInputAdmissionParams,
    ) -> TalosResult<AgentEventCommit>;

    /// Atomically move one exact active Turn to Cancelling.
    async fn begin_cancellation(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<AgentCancellationAdmission>;

    /// Restore the pre-cancellation state when runtime interruption is rejected.
    async fn restore_cancellation(
        &self,
        admission: AgentCancellationAdmission,
    ) -> TalosResult<AgentActiveTurnRecord>;

    /// Atomically append waiting settlement and bind Provider continuation state.
    async fn commit_waiting_settlement(
        &self,
        event: AgentEvent,
        continuation: AgentContinuationRef,
        expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit>;

    /// Atomically claim one waiting continuation and append accepted User input.
    async fn claim_continuation(
        &self,
        params: ClaimContinuationParams,
    ) -> TalosResult<AgentEventCommit>;

    /// Atomically append the unique terminal fact and remove nonterminal indexes.
    async fn commit_terminal_settlement(
        &self,
        params: CommitTerminalSettlementParams,
    ) -> TalosResult<AgentEventRecord>;

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

    /// Read the current continuation ownership for one exact logical Turn.
    async fn get_agent_continuation_binding(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<AgentContinuationBindingRecord>>;
}

fn validate_initial_admission(events: &[AgentEvent]) -> TalosResult<(AgentTurnRef, String)> {
    let first = events
        .first()
        .ok_or_else(|| execution_conflict("initial admission cannot be empty"))?;
    if !matches!(first.payload, AgentEventPayload::TurnStarted(_)) {
        return Err(execution_conflict(
            "initial admission must begin with TurnStarted",
        ));
    }
    if first.sequence != 1 {
        return Err(execution_conflict(
            "initial admission must start at Agent event sequence 1",
        ));
    }
    validate_event_batch(events)?;
    if events.iter().skip(1).any(|event| {
        matches!(
            event.payload,
            AgentEventPayload::TurnStarted(_)
                | AgentEventPayload::WaitingForInput(_)
                | AgentEventPayload::TurnCompleted(_)
                | AgentEventPayload::TurnFailed(_)
        )
    }) {
        return Err(execution_conflict(
            "initial admission contains a waiting or terminal lifecycle event",
        ));
    }
    let user_messages = events
        .iter()
        .filter_map(|event| match &event.payload {
            AgentEventPayload::MessageCompleted(message)
                if message.role == AgentMessageRole::User =>
            {
                Some(message.message_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [message_id] = user_messages.as_slice() else {
        return Err(execution_conflict(
            "initial admission requires exactly one completed User message",
        ));
    };
    Ok((first.identity.clone(), message_id.clone()))
}

fn validate_event_batch(events: &[AgentEvent]) -> TalosResult<()> {
    let first = events
        .first()
        .ok_or_else(|| execution_conflict("Agent event batch cannot be empty"))?;
    for (offset, event) in events.iter().enumerate() {
        if event.identity != first.identity
            || event.sequence != first.sequence.saturating_add(offset as u64)
        {
            return Err(execution_conflict(
                "Agent event batch identity or sequence is inconsistent",
            ));
        }
    }
    Ok(())
}

fn validate_user_admission(events: &[AgentEvent]) -> TalosResult<(AgentTurnRef, String)> {
    validate_event_batch(events)?;
    if events.iter().any(|event| {
        matches!(
            event.payload,
            AgentEventPayload::TurnStarted(_)
                | AgentEventPayload::WaitingForInput(_)
                | AgentEventPayload::TurnCompleted(_)
                | AgentEventPayload::TurnFailed(_)
        )
    }) {
        return Err(execution_conflict(
            "User admission contains a Turn lifecycle event",
        ));
    }
    let completed_user_messages = events
        .iter()
        .filter_map(|event| match &event.payload {
            AgentEventPayload::MessageCompleted(message)
                if message.role == AgentMessageRole::User =>
            {
                Some(message.message_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [message_id] = completed_user_messages.as_slice() else {
        return Err(execution_conflict(
            "User admission requires exactly one completed User message",
        ));
    };
    Ok((
        events
            .first()
            .expect("validated User admission is non-empty")
            .identity
            .clone(),
        message_id.clone(),
    ))
}

fn validate_progress_event(event: &AgentEvent) -> TalosResult<()> {
    if matches!(
        event.payload,
        AgentEventPayload::TurnStarted(_)
            | AgentEventPayload::WaitingForInput(_)
            | AgentEventPayload::TurnCompleted(_)
            | AgentEventPayload::TurnFailed(_)
    ) {
        return Err(execution_conflict(
            "progress commit cannot persist admission, waiting, or terminal events",
        ));
    }
    Ok(())
}

fn execution_conflict(message: impl Into<String>) -> TalosError {
    TalosError::new(talos_error::ErrorCode::Conflict, message.into())
}

#[cfg(test)]
#[path = "agent_execution_tests.rs"]
mod tests;
