use async_trait::async_trait;
use talos_agent_api::{AgentContinuationRef, AgentEvent, AgentTurnRef};
use talos_error::TalosResult;

use super::record::{load_continuation_binding, parse_active_turn, parse_event_record};
use super::{
    admission, continuation, settlement, AgentActiveTurnRecord, AgentCancellationAdmission,
    AgentContinuationBindingRecord, AgentEventCommit, AgentEventRecord, AgentExecutionStore,
    ClaimContinuationParams, CommandLease, CommandReservation, CommitInitialAdmissionParams,
    CommitRunningInputAdmissionParams, CommitTerminalSettlementParams, IdempotencyReservation,
};
use crate::session::DatabaseSessionStore;

const MAX_EVENT_READ_LIMIT: usize = 10_000;

#[async_trait]
impl AgentExecutionStore for DatabaseSessionStore {
    fn supports_history_supplements(&self) -> bool {
        true
    }

    async fn append_history_supplement(
        &self,
        supplement: talos_agent_api::trajectory::SessionHistorySupplement,
    ) -> TalosResult<()> {
        super::supplement::append(self, supplement).await
    }

    async fn read_terminal_history_supplement(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<talos_agent_api::trajectory::SessionHistorySupplement>> {
        super::supplement::read_terminal(self, identity).await
    }

    async fn read_execution_payload(
        &self,
        identity: &AgentTurnRef,
        payload_id: &str,
    ) -> TalosResult<Option<String>> {
        super::supplement::read_payload(self, identity, payload_id).await
    }

    async fn get_shared_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_agent_api::TimelineActiveTurn>> {
        let mut tx = self.storage.begin_read_transaction().await?;
        let Some(adapter) = self
            .sync
            .resource(
                &mut *tx,
                &talos_sync::SyncResourceKey::Session {
                    session_id: session_id.into(),
                },
            )
            .await?
        else {
            tx.commit().await?;
            return Ok(None);
        };
        let rows = tx.query("SELECT event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json FROM agent_events WHERE session_id = ? AND sequence IS NOT NULL ORDER BY event_seq DESC LIMIT 1", &[session_id]).await?;
        let Some(row) = rows.first() else {
            tx.commit().await?;
            return Ok(None);
        };
        let last = parse_event_record(row)?;
        if matches!(
            last.payload,
            talos_agent_api::AgentEventPayload::TurnCompleted(_)
                | talos_agent_api::AgentEventPayload::TurnFailed(_)
        ) {
            tx.commit().await?;
            return Ok(None);
        }
        let metadata = adapter
            .sync
            .active_turn_metadata(&mut *tx, adapter.binding(), &last.identity)
            .await?;
        let key =
            crate::sync::session::key_at(&adapter, &mut *tx, session_id, last.event_seq).await?;
        let (state, updated_at) =
            match metadata.filter(|meta| Some(&meta.last_event) == key.as_ref()) {
                Some(meta) => (meta.state, meta.updated_at),
                None => (
                    if matches!(
                        last.payload,
                        talos_agent_api::AgentEventPayload::WaitingForInput(_)
                    ) {
                        talos_agent_api::ActiveTurnState::WaitingForInput
                    } else {
                        talos_agent_api::ActiveTurnState::Running
                    },
                    last.persisted_at,
                ),
            };
        let initial_message = tx.query("SELECT json_extract(payload_json, '$.data.messageId') AS message_id FROM agent_events WHERE session_id = ? AND turn_id = ? AND json_extract(payload_json, '$.type') = 'messageCompleted' AND json_extract(payload_json, '$.data.role') = 'user' ORDER BY sequence LIMIT 1", &[session_id, &last.identity.turn_id]).await?.first().and_then(|r| r.get_string("message_id"));
        tx.commit().await?;
        Ok(Some(talos_agent_api::TimelineActiveTurn {
            identity: last.identity,
            initial_message_id: initial_message,
            state,
            last_event_id: talos_types::TimelineEventId::from_seq(last.event_seq).to_string(),
            updated_at,
        }))
    }

    async fn append_timeline_notice(
        &self,
        notice: talos_agent_api::TimelineSystemNotice,
    ) -> TalosResult<super::TimelineEventRecord> {
        super::notice::append(self, notice).await
    }

    async fn list_timeline_events_after(
        &self,
        session_id: &str,
        after_event_seq: i64,
        limit: usize,
    ) -> TalosResult<Vec<super::TimelineEventRecord>> {
        let after = after_event_seq.to_string();
        let limit = limit.clamp(1, MAX_EVENT_READ_LIMIT).to_string();
        self.storage.query("SELECT event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json FROM agent_events WHERE session_id = ? AND event_seq > ? ORDER BY event_seq ASC LIMIT ?", &[session_id, &after, &limit]).await?
            .iter().map(super::notice::parse).collect()
    }

    async fn get_timeline_event(
        &self,
        event_seq: i64,
    ) -> TalosResult<Option<super::TimelineEventRecord>> {
        self.storage
            .query(
                "SELECT event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json FROM agent_events WHERE event_seq = ?",
                &[&event_seq.to_string()],
            )
            .await?
            .first()
            .map(super::notice::parse)
            .transpose()
    }

    async fn get_latest_timeline_event_seq(&self, session_id: &str) -> TalosResult<Option<i64>> {
        Ok(self
            .storage
            .query(
                "SELECT MAX(event_seq) AS event_seq FROM agent_events WHERE session_id = ?",
                &[session_id],
            )
            .await?
            .first()
            .and_then(|row| row.get_i64("event_seq")))
    }

    async fn list_turn_notices(
        &self,
        identities: &[AgentTurnRef],
        through_event_seq: i64,
    ) -> TalosResult<Vec<super::TimelineEventRecord>> {
        super::notice::list(self, identities, through_event_seq).await
    }

    async fn reserve_command(
        &self,
        reservation: CommandReservation,
    ) -> TalosResult<IdempotencyReservation> {
        super::idempotency::reserve(self, reservation).await
    }

    async fn release_command(&self, lease: CommandLease) -> TalosResult<()> {
        super::idempotency::release(self, lease).await
    }

    async fn commit_initial_admission(
        &self,
        params: CommitInitialAdmissionParams,
    ) -> TalosResult<AgentEventCommit> {
        admission::commit_initial(self, params).await
    }

    async fn commit_progress_event(
        &self,
        event: AgentEvent,
        expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit> {
        admission::commit_progress(self, event, expected_event_seq).await
    }

    async fn commit_maintenance_admission(
        &self,
        event: AgentEvent,
    ) -> TalosResult<AgentEventCommit> {
        admission::commit_maintenance(self, event).await
    }

    async fn commit_progress_events(
        &self,
        events: Vec<AgentEvent>,
        expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit> {
        admission::commit_progress_batch(self, events, expected_event_seq).await
    }

    async fn commit_running_input_admission(
        &self,
        params: CommitRunningInputAdmissionParams,
    ) -> TalosResult<AgentEventCommit> {
        admission::commit_running_input(self, params).await
    }

    async fn begin_cancellation(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<AgentCancellationAdmission> {
        super::cancellation::begin(self, identity).await
    }

    async fn restore_cancellation(
        &self,
        admission: AgentCancellationAdmission,
    ) -> TalosResult<AgentActiveTurnRecord> {
        super::cancellation::restore(self, admission).await
    }

    async fn commit_waiting_settlement(
        &self,
        event: AgentEvent,
        continuation: AgentContinuationRef,
        expected_event_seq: i64,
    ) -> TalosResult<AgentEventCommit> {
        settlement::commit_waiting(self, event, continuation, expected_event_seq).await
    }

    async fn claim_continuation(
        &self,
        params: ClaimContinuationParams,
    ) -> TalosResult<AgentEventCommit> {
        continuation::claim(self, params).await
    }

    async fn commit_terminal_settlement(
        &self,
        params: CommitTerminalSettlementParams,
    ) -> TalosResult<AgentEventRecord> {
        settlement::commit_terminal(self, params).await
    }

    async fn list_agent_turn_events(
        &self,
        identity: &AgentTurnRef,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        let limit = limit.clamp(1, MAX_EVENT_READ_LIMIT).to_string();
        let rows = self
            .storage
            .query(
                r#"
                SELECT event_seq, session_id, turn_id, sequence,
                       occurred_at, persisted_at, payload_json
                FROM agent_events
                WHERE session_id = ? AND turn_id = ? AND sequence IS NOT NULL
                ORDER BY sequence ASC
                LIMIT ?
                "#,
                &[
                    identity.session_id.as_str(),
                    identity.turn_id.as_str(),
                    limit.as_str(),
                ],
            )
            .await?;
        rows.iter().map(parse_event_record).collect()
    }

    async fn list_agent_events_after(
        &self,
        session_id: &str,
        after_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        let after_event_seq = after_event_seq.unwrap_or_default().to_string();
        let limit = limit.clamp(1, MAX_EVENT_READ_LIMIT).to_string();
        let rows = self
            .storage
            .query(
                r#"
                SELECT event_seq, session_id, turn_id, sequence,
                       occurred_at, persisted_at, payload_json
                FROM agent_events
                WHERE session_id = ? AND event_seq > ? AND sequence IS NOT NULL
                ORDER BY event_seq ASC
                LIMIT ?
                "#,
                &[session_id, after_event_seq.as_str(), limit.as_str()],
            )
            .await?;
        rows.iter().map(parse_event_record).collect()
    }

    async fn get_agent_event(&self, event_seq: i64) -> TalosResult<Option<AgentEventRecord>> {
        let event_seq = event_seq.to_string();
        let rows = self
            .storage
            .query(
                r#"
                SELECT event_seq, session_id, turn_id, sequence,
                       occurred_at, persisted_at, payload_json
                FROM agent_events
                WHERE event_seq = ? AND sequence IS NOT NULL
                "#,
                &[event_seq.as_str()],
            )
            .await?;
        rows.first().map(parse_event_record).transpose()
    }

    async fn get_latest_agent_event_seq(&self, session_id: &str) -> TalosResult<Option<i64>> {
        let rows = self
            .storage
            .query(
                "SELECT MAX(event_seq) AS event_seq FROM agent_events WHERE session_id = ? AND sequence IS NOT NULL",
                &[session_id],
            )
            .await?;
        Ok(rows.first().and_then(|row| row.get_i64("event_seq")))
    }

    async fn get_latest_agent_terminal_event(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentEventRecord>> {
        let rows = self
            .storage
            .query(
                r#"
                SELECT event_seq, session_id, turn_id, sequence,
                       occurred_at, persisted_at, payload_json
                FROM agent_events
                WHERE session_id = ?
                  AND json_extract(payload_json, '$.type') IN ('turnCompleted', 'turnFailed')
                ORDER BY event_seq DESC
                LIMIT 1
                "#,
                &[session_id],
            )
            .await?;
        rows.first().map(parse_event_record).transpose()
    }

    async fn list_latest_agent_terminal_events(
        &self,
        session_id: &str,
        before_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        let before_event_seq = before_event_seq.unwrap_or(i64::MAX).to_string();
        let limit = limit.clamp(1, MAX_EVENT_READ_LIMIT).to_string();
        let rows = self
            .storage
            .query(
                r#"
                SELECT event_seq, session_id, turn_id, sequence,
                       occurred_at, persisted_at, payload_json
                FROM (
                    SELECT event_seq, session_id, turn_id, sequence,
                           occurred_at, persisted_at, payload_json
                    FROM agent_events
                    WHERE session_id = ?
                      AND event_seq < ?
                      AND json_extract(payload_json, '$.type')
                          IN ('turnCompleted', 'turnFailed')
                    ORDER BY event_seq DESC
                    LIMIT ?
                ) AS latest_terminal_events
                ORDER BY event_seq ASC
                "#,
                &[session_id, before_event_seq.as_str(), limit.as_str()],
            )
            .await?;
        rows.iter().map(parse_event_record).collect()
    }

    async fn get_agent_terminal_event_by_initial_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> TalosResult<Option<AgentEventRecord>> {
        let rows = self
            .storage
            .query(
                r#"
                WITH initial_turn AS (
                    SELECT message.turn_id
                    FROM agent_events AS message
                    WHERE message.session_id = ?
                      AND json_extract(message.payload_json, '$.type') = 'messageCompleted'
                      AND json_extract(message.payload_json, '$.data.messageId') = ?
                      AND json_extract(message.payload_json, '$.data.role') = 'user'
                      AND NOT EXISTS (
                          SELECT 1
                          FROM agent_events AS earlier
                          WHERE earlier.session_id = message.session_id
                            AND earlier.turn_id = message.turn_id
                            AND earlier.sequence < message.sequence
                            AND json_extract(earlier.payload_json, '$.type') = 'messageCompleted'
                            AND json_extract(earlier.payload_json, '$.data.role') = 'user'
                      )
                )
                SELECT terminal.event_seq, terminal.session_id, terminal.turn_id,
                       terminal.sequence, terminal.occurred_at, terminal.persisted_at,
                       terminal.payload_json
                FROM agent_events AS terminal
                JOIN initial_turn ON initial_turn.turn_id = terminal.turn_id
                WHERE terminal.session_id = ?
                  AND json_extract(terminal.payload_json, '$.type')
                      IN ('turnCompleted', 'turnFailed')
                ORDER BY terminal.event_seq DESC
                LIMIT 2
                "#,
                &[session_id, message_id, session_id],
            )
            .await?;
        if rows.len() > 1 {
            return Err(talos_error::TalosError::internal(
                "initial Agent message resolves to multiple terminal Turns",
            ));
        }
        rows.first().map(parse_event_record).transpose()
    }

    async fn get_agent_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentActiveTurnRecord>> {
        let rows = self
            .storage
            .query(
                r#"
                SELECT active.session_id, active.turn_id, active.initial_message_id,
                       active.state, active.last_event_seq,
                       binding.action_id AS current_action_id, active.updated_at
                FROM agent_active_turns AS active
                LEFT JOIN agent_continuation_bindings AS binding
                  ON binding.session_id = active.session_id
                 AND binding.turn_id = active.turn_id
                 AND active.state = 'waiting_for_input'
                 AND binding.status = 'waiting'
                WHERE active.session_id = ?
                "#,
                &[session_id],
            )
            .await?;
        rows.first().map(parse_active_turn).transpose()
    }

    async fn get_agent_continuation_binding(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<AgentContinuationBindingRecord>> {
        let mut transaction = self.storage.begin_read_transaction().await?;
        let binding = load_continuation_binding(transaction.as_mut(), identity).await?;
        transaction.rollback().await?;
        Ok(binding)
    }
}
