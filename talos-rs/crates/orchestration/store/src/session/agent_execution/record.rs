use talos_agent_api::{ActiveTurnState, AgentEvent, AgentEventPayload, AgentTurnRef};
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};

use super::{AgentActiveTurnRecord, AgentEventRecord};

pub(crate) async fn append_events(
    transaction: &mut dyn StorageTransaction,
    events: Vec<AgentEvent>,
) -> TalosResult<Vec<AgentEventRecord>> {
    write_events(
        transaction,
        events.into_iter().map(|event| (None, event)).collect(),
    )
    .await
}

#[derive(Clone, Copy)]
pub(crate) struct PreservedEventPosition {
    pub(crate) event_seq: i64,
    pub(crate) persisted_at: i64,
}

/// Restore the original product cursor so clients and management receipts keep their identities.
pub(crate) async fn restore_event(
    transaction: &mut dyn StorageTransaction,
    event: AgentEvent,
    position: PreservedEventPosition,
) -> TalosResult<AgentEventRecord> {
    write_events(transaction, vec![(Some(position), event)])
        .await?
        .pop()
        .ok_or_else(|| TalosError::internal("Restored Agent event is unavailable."))
}

async fn write_events(
    transaction: &mut dyn StorageTransaction,
    events: Vec<(Option<PreservedEventPosition>, AgentEvent)>,
) -> TalosResult<Vec<AgentEventRecord>> {
    let persisted_at = chrono::Utc::now().timestamp();
    let mut records = Vec::with_capacity(events.len());
    for (position, event) in events {
        let payload_json = serde_json::to_string(&event.payload).map_err(|error| {
            TalosError::internal_with_source(
                "serialize Agent event; technical details were withheld.",
                error,
            )
        })?;
        let sequence = event.sequence.to_string();
        let occurred_at = event.occurred_at.to_string();
        let persisted_at_param = position
            .map_or(persisted_at, |position| position.persisted_at)
            .to_string();
        let rows = transaction
            .query(
                r#"
                INSERT INTO agent_events (
                    event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json,
                    history_generation
                )
                VALUES (NULLIF(?1, ''), ?2, ?3, ?4, ?5, ?6, ?7,
                        COALESCE((SELECT history_generation FROM sessions WHERE session_key = ?2), 0))
                RETURNING event_seq, session_id, turn_id, sequence,
                          occurred_at, persisted_at, payload_json
                "#,
                &[
                    &position.map(|position| position.event_seq.to_string()).unwrap_or_default(),
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

pub(crate) async fn load_active_turn(
    transaction: &mut dyn StorageTransaction,
    session_id: &str,
) -> TalosResult<Option<AgentActiveTurnRecord>> {
    let rows = transaction
        .query(
            r#"
            SELECT active.session_id, active.turn_id, active.initial_message_id,
                   active.state, active.cancellation_request_id, active.last_event_seq, event.payload_json AS active_payload_json,
                   active.updated_at
            FROM agent_active_turns AS active
            LEFT JOIN visible_agent_events AS event ON event.event_seq = active.last_event_seq
            WHERE active.session_id = ?
            "#,
            &[session_id],
        )
        .await?;
    rows.first().map(parse_active_turn).transpose()
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

pub(crate) fn parse_active_turn_state(row: &Row) -> TalosResult<ActiveTurnState> {
    Ok(match required_string(row, "state")?.as_str() {
        "running" => ActiveTurnState::Running,
        "waiting_for_input" => ActiveTurnState::WaitingForInput,
        "cancelling" => ActiveTurnState::Cancelling,
        state => {
            return Err(TalosError::internal(
                "unknown Agent Turn state; technical details were withheld.",
            )
            .context(format!("unknown Agent Turn state: {state}")))
        }
    })
}

pub(crate) fn parse_active_turn(row: &Row) -> TalosResult<AgentActiveTurnRecord> {
    let state = parse_active_turn_state(row)?;
    Ok(AgentActiveTurnRecord {
        identity: AgentTurnRef::new(
            required_string(row, "session_id")?,
            required_string(row, "turn_id")?,
        ),
        initial_message_id: row
            .get_string("initial_message_id")
            .filter(|value| !value.trim().is_empty()),
        state,
        cancellation_requested: row.get_string("cancellation_request_id").is_some(),
        last_event_seq: required_i64(row, "last_event_seq")?,
        current_action_id: if state == ActiveTurnState::WaitingForInput {
            let raw = row.get_string("active_payload_json").ok_or_else(|| {
                TalosError::internal("Waiting product projection has no committed event")
            })?;
            match serde_json::from_str::<AgentEventPayload>(&raw).map_err(|error| {
                TalosError::internal_with_source("Read waiting product event", error)
            })? {
                AgentEventPayload::WaitingForInput(input) => Some(input.action_id().to_owned()),
                _ => {
                    return Err(TalosError::internal(
                        "Waiting product projection has no waiting event",
                    ))
                }
            }
        } else {
            None
        },
        updated_at: required_i64(row, "updated_at")?,
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
