use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use talos_agent_api::{AgentContinuationRef, AgentEvent, AgentEventPayload, AgentTurnRef};
use talos_error::{TalosError, TalosResult};
use talos_store::session::{
    AgentActiveTurnRecord, AgentCancellationAdmission, AgentContinuationBindingRecord,
    AgentEventCommit, AgentEventRecord, AgentExecutionStore, ClaimContinuationParams, CommandLease,
    CommandReservation, CommitInitialAdmissionParams, CommitRunningInputAdmissionParams,
    CommitTerminalSettlementParams, IdempotencyReservation,
};

#[derive(Default)]
pub(super) struct RecordingAgentExecutionStore {
    events: Mutex<Vec<AgentEventRecord>>,
    active_turns: Mutex<HashMap<String, AgentActiveTurnRecord>>,
}

impl RecordingAgentExecutionStore {
    pub(super) fn append_event(&self, event: AgentEvent) -> AgentEventRecord {
        let mut events = self.events.lock().expect("event fixture lock");
        let event_seq = events.len() as i64 + 1;
        let record = AgentEventRecord {
            event_seq,
            identity: event.identity,
            sequence: event.sequence,
            occurred_at: event.occurred_at,
            persisted_at: 1_000 + event_seq,
            payload: event.payload,
        };
        events.push(record.clone());
        record
    }

    pub(super) fn set_active_turn(&self, active_turn: AgentActiveTurnRecord) {
        self.active_turns
            .lock()
            .expect("active Turn fixture lock")
            .insert(active_turn.identity.session_id.clone(), active_turn);
    }
}

#[async_trait]
impl AgentExecutionStore for RecordingAgentExecutionStore {
    async fn reserve_command(
        &self,
        _reservation: CommandReservation,
    ) -> TalosResult<IdempotencyReservation> {
        Err(out_of_scope())
    }

    async fn release_command(&self, _lease: CommandLease) -> TalosResult<()> {
        Err(out_of_scope())
    }

    async fn commit_initial_admission(
        &self,
        _params: CommitInitialAdmissionParams,
    ) -> TalosResult<AgentEventCommit> {
        Err(out_of_scope())
    }

    async fn commit_maintenance_admission(
        &self,
        _event: AgentEvent,
    ) -> TalosResult<AgentEventCommit> {
        Err(out_of_scope())
    }

    async fn commit_progress_event(
        &self,
        _event: AgentEvent,
        _expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit> {
        Err(out_of_scope())
    }

    async fn commit_running_input_admission(
        &self,
        _params: CommitRunningInputAdmissionParams,
    ) -> TalosResult<AgentEventCommit> {
        Err(out_of_scope())
    }

    async fn begin_cancellation(
        &self,
        _identity: &AgentTurnRef,
    ) -> TalosResult<AgentCancellationAdmission> {
        Err(out_of_scope())
    }

    async fn restore_cancellation(
        &self,
        _admission: AgentCancellationAdmission,
    ) -> TalosResult<AgentActiveTurnRecord> {
        Err(out_of_scope())
    }

    async fn commit_waiting_settlement(
        &self,
        _event: AgentEvent,
        _continuation: AgentContinuationRef,
        _expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit> {
        Err(out_of_scope())
    }

    async fn claim_continuation(
        &self,
        _params: ClaimContinuationParams,
    ) -> TalosResult<AgentEventCommit> {
        Err(out_of_scope())
    }

    async fn commit_terminal_settlement(
        &self,
        _params: CommitTerminalSettlementParams,
    ) -> TalosResult<AgentEventRecord> {
        Err(out_of_scope())
    }

    async fn list_agent_turn_events(
        &self,
        identity: &AgentTurnRef,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        Ok(self
            .events
            .lock()
            .expect("event fixture lock")
            .iter()
            .filter(|event| event.identity == *identity)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn list_agent_events_after(
        &self,
        session_id: &str,
        after_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        let after_event_seq = after_event_seq.unwrap_or_default();
        Ok(self
            .events
            .lock()
            .expect("event fixture lock")
            .iter()
            .filter(|event| {
                event.identity.session_id == session_id && event.event_seq > after_event_seq
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn get_agent_event(&self, event_seq: i64) -> TalosResult<Option<AgentEventRecord>> {
        Ok(self
            .events
            .lock()
            .expect("event fixture lock")
            .iter()
            .find(|event| event.event_seq == event_seq)
            .cloned())
    }

    async fn get_latest_agent_event_seq(&self, session_id: &str) -> TalosResult<Option<i64>> {
        Ok(self
            .events
            .lock()
            .expect("event fixture lock")
            .iter()
            .rev()
            .find(|event| event.identity.session_id == session_id)
            .map(|event| event.event_seq))
    }

    async fn get_latest_agent_terminal_event(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentEventRecord>> {
        Ok(self
            .events
            .lock()
            .expect("event fixture lock")
            .iter()
            .rev()
            .find(|event| {
                event.identity.session_id == session_id
                    && matches!(
                        event.payload,
                        AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_)
                    )
            })
            .cloned())
    }

    async fn list_latest_agent_terminal_events(
        &self,
        session_id: &str,
        before_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        let before_event_seq = before_event_seq.unwrap_or(i64::MAX);
        let mut events = self
            .events
            .lock()
            .expect("event fixture lock")
            .iter()
            .rev()
            .filter(|event| {
                event.identity.session_id == session_id
                    && event.event_seq < before_event_seq
                    && matches!(
                        event.payload,
                        AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_)
                    )
            })
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        events.reverse();
        Ok(events)
    }

    async fn get_agent_terminal_event_by_initial_message(
        &self,
        _session_id: &str,
        _message_id: &str,
    ) -> TalosResult<Option<AgentEventRecord>> {
        Ok(None)
    }

    async fn get_agent_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentActiveTurnRecord>> {
        Ok(self
            .active_turns
            .lock()
            .expect("active Turn fixture lock")
            .get(session_id)
            .cloned())
    }

    async fn get_agent_continuation_binding(
        &self,
        _identity: &AgentTurnRef,
    ) -> TalosResult<Option<AgentContinuationBindingRecord>> {
        Ok(None)
    }
}

fn out_of_scope() -> TalosError {
    TalosError::internal("canonical execution mutation is outside this HTTP test fixture")
}
