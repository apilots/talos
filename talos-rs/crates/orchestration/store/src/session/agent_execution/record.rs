use talos_agent_api::{ActiveTurnState, AgentContinuationRef, AgentEvent, AgentTurnRef};
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};

use super::{
    execution_conflict, AgentActiveTurnRecord, AgentContinuationBindingRecord,
    AgentContinuationBindingStatus, AgentEventRecord,
};

pub(super) async fn append_events(
    transaction: &mut dyn StorageTransaction,
    events: Vec<AgentEvent>,
) -> TalosResult<Vec<AgentEventRecord>> {
    let persisted_at = chrono::Utc::now().timestamp();
    let mut records = Vec::with_capacity(events.len());
    for event in events {
        let payload_json = serde_json::to_string(&event.payload).map_err(|error| {
            TalosError::internal_with_source(
                "serialize Agent event; technical details were withheld.",
                error,
            )
        })?;
        let sequence = event.sequence.to_string();
        let occurred_at = event.occurred_at.to_string();
        let persisted_at_param = persisted_at.to_string();
        let rows = transaction
            .query(
                r#"
                INSERT INTO agent_events (
                    session_id, turn_id, sequence, occurred_at, persisted_at, payload_json
                ) VALUES (?, ?, ?, ?, ?, ?)
                RETURNING event_seq, session_id, turn_id, sequence,
                          occurred_at, persisted_at, payload_json
                "#,
                &[
                    event.identity.session_id.as_str(),
                    event.identity.turn_id.as_str(),
                    sequence.as_str(),
                    occurred_at.as_str(),
                    persisted_at_param.as_str(),
                    payload_json.as_str(),
                ],
            )
            .await?;
        let record = rows
            .first()
            .ok_or_else(|| TalosError::internal("Agent event insert returned no record"))
            .and_then(parse_event_record)?;
        records.push(record);
    }
    Ok(records)
}

pub(super) async fn load_active_turn(
    transaction: &mut dyn StorageTransaction,
    session_id: &str,
) -> TalosResult<Option<AgentActiveTurnRecord>> {
    let rows = transaction
        .query(
            r#"
            SELECT active.session_id, active.turn_id, active.initial_message_id,
                   active.state, active.last_event_seq, binding.action_id AS current_action_id,
                   active.updated_at
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

pub(super) async fn load_event_sequence(
    transaction: &mut dyn StorageTransaction,
    event_seq: i64,
) -> TalosResult<u64> {
    let event_seq = event_seq.to_string();
    let rows = transaction
        .query(
            "SELECT sequence FROM agent_events WHERE event_seq = ?",
            &[event_seq.as_str()],
        )
        .await?;
    rows.first()
        .and_then(|row| row.get_i64("sequence"))
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| execution_conflict("active Agent Turn tail event does not exist"))
}

pub(super) async fn load_continuation_binding(
    transaction: &mut dyn StorageTransaction,
    identity: &AgentTurnRef,
) -> TalosResult<Option<AgentContinuationBindingRecord>> {
    let rows = transaction
        .query(
            r#"
            SELECT session_id, turn_id, action_id, provider_ref_json,
                   expected_event_seq, status, claim_id
            FROM agent_continuation_bindings
            WHERE session_id = ? AND turn_id = ?
            "#,
            &[identity.session_id.as_str(), identity.turn_id.as_str()],
        )
        .await?;
    rows.first().map(parse_continuation_binding).transpose()
}

pub(super) async fn load_event_range(
    transaction: &mut dyn StorageTransaction,
    identity: &AgentTurnRef,
    first_sequence: u64,
    last_sequence: u64,
) -> TalosResult<Vec<AgentEventRecord>> {
    let first_sequence = first_sequence.to_string();
    let last_sequence = last_sequence.to_string();
    let rows = transaction
        .query(
            r#"
            SELECT event_seq, session_id, turn_id, sequence,
                   occurred_at, persisted_at, payload_json
            FROM agent_events
            WHERE session_id = ? AND turn_id = ?
              AND sequence >= ? AND sequence <= ?
            ORDER BY sequence ASC
            "#,
            &[
                identity.session_id.as_str(),
                identity.turn_id.as_str(),
                first_sequence.as_str(),
                last_sequence.as_str(),
            ],
        )
        .await?;
    rows.iter().map(parse_event_record).collect()
}

pub(crate) fn parse_event_record(row: &Row) -> TalosResult<AgentEventRecord> {
    let payload = row
        .get_string("payload_json")
        .ok_or_else(|| TalosError::internal("Agent event record is missing payload"))
        .and_then(|raw| {
            serde_json::from_str(&raw).map_err(|error| {
                TalosError::internal_with_source(
                    "parse Agent event; technical details were withheld.",
                    error,
                )
            })
        })?;
    Ok(AgentEventRecord {
        event_seq: required_i64(row, "event_seq")?,
        identity: AgentTurnRef::new(
            required_string(row, "session_id")?,
            required_string(row, "turn_id")?,
        ),
        sequence: required_i64(row, "sequence")?
            .try_into()
            .map_err(|_| TalosError::internal("Agent event sequence is negative"))?,
        occurred_at: required_i64(row, "occurred_at")?,
        persisted_at: required_i64(row, "persisted_at")?,
        payload,
    })
}

pub(crate) fn parse_active_turn(row: &Row) -> TalosResult<AgentActiveTurnRecord> {
    let state = match required_string(row, "state")?.as_str() {
        "running" => ActiveTurnState::Running,
        "waiting_for_input" => ActiveTurnState::WaitingForInput,
        "cancelling" => ActiveTurnState::Cancelling,
        state => {
            return Err(TalosError::internal(
                "unknown Agent Turn state; technical details were withheld.",
            )
            .context(format!("unknown Agent Turn state: {state}")))
        }
    };
    Ok(AgentActiveTurnRecord {
        identity: AgentTurnRef::new(
            required_string(row, "session_id")?,
            required_string(row, "turn_id")?,
        ),
        initial_message_id: row
            .get_string("initial_message_id")
            .filter(|value| !value.trim().is_empty()),
        state,
        last_event_seq: required_i64(row, "last_event_seq")?,
        current_action_id: row
            .get_string("current_action_id")
            .filter(|value| !value.trim().is_empty()),
        updated_at: required_i64(row, "updated_at")?,
    })
}

pub(crate) fn parse_continuation_binding(row: &Row) -> TalosResult<AgentContinuationBindingRecord> {
    let claim_id = row
        .get_string("claim_id")
        .filter(|value| !value.trim().is_empty());
    let status = match (required_string(row, "status")?.as_str(), claim_id) {
        ("waiting", None) => AgentContinuationBindingStatus::Waiting,
        ("claimed", Some(claim_id)) => AgentContinuationBindingStatus::Claimed { claim_id },
        (status, _) => {
            return Err(TalosError::internal(
                "invalid Agent continuation binding status; technical details were withheld.",
            )
            .context(format!(
                "invalid Agent continuation binding status: {status}"
            )))
        }
    };
    let provider_ref = row
        .get_string("provider_ref_json")
        .ok_or_else(|| TalosError::internal("Agent continuation binding is missing Provider ref"))
        .and_then(|raw| {
            serde_json::from_str::<AgentContinuationRef>(&raw).map_err(|error| {
                TalosError::internal_with_source(
                    "parse Agent continuation reference; technical details were withheld.",
                    error,
                )
            })
        })?;
    Ok(AgentContinuationBindingRecord {
        identity: AgentTurnRef::new(
            required_string(row, "session_id")?,
            required_string(row, "turn_id")?,
        ),
        action_id: required_string(row, "action_id")?,
        provider_ref,
        expected_event_seq: required_i64(row, "expected_event_seq")?,
        status,
    })
}

fn required_string(row: &Row, column: &str) -> TalosResult<String> {
    row.get_string(column).ok_or_else(|| {
        TalosError::internal("Agent execution record is missing; technical details were withheld.")
            .context(format!("Agent execution record is missing {column}"))
    })
}

fn required_i64(row: &Row, column: &str) -> TalosResult<i64> {
    row.get_i64(column).ok_or_else(|| {
        TalosError::internal("Agent execution record is missing; technical details were withheld.")
            .context(format!("Agent execution record is missing {column}"))
    })
}
