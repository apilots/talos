//! Product projections of committed Worker facts, with one transactional source cursor.
use super::agent_execution::{
    record::{append_events, load_active_turn, parse_event_record},
    settlement::{update_compacted_session, update_completed_turn_session},
};
use super::{AgentActiveTurnRecord, AgentEventRecord, DatabaseSessionStore};
use agent_protocol::{
    AgentCommittedEvent, AgentExecutionSettlement, AgentSessionEventCursor,
    AgentSessionEventsResult,
};
use sha2::{Digest, Sha256};
use talos_agent_api::{AgentEventPayload, AgentMessageRole};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;

#[path = "worker_projection_metadata.rs"]
mod metadata;

pub struct WorkerProjectionParams {
    pub execution_node_id: String,
    pub session_id: String,
    pub after_cursor: Option<AgentSessionEventCursor>,
    pub page: AgentSessionEventsResult,
}

pub struct WorkerProjectionCommit {
    pub events: Vec<AgentEventRecord>,
    pub active_turn: Option<AgentActiveTurnRecord>,
    pub cursor: AgentSessionEventCursor,
}

pub struct WorkerSettlementRecord {
    pub execution_purpose: agent_protocol::AgentExecutionPurpose,
    pub cursor: AgentSessionEventCursor,
    pub event: AgentEventRecord,
    pub settlement: AgentExecutionSettlement,
    pub command: Option<agent_protocol::AgentCommandIdentity>,
    pub execution_model: Option<talos_types::ModelRef>,
}

pub(super) async fn cursor(
    store: &DatabaseSessionStore,
    session_id: &str,
) -> TalosResult<Option<AgentSessionEventCursor>> {
    let rows = store.storage.query("SELECT history_generation, worker_offset FROM worker_projection_cursors WHERE session_id=?", &[session_id]).await?;
    rows.first()
        .map(|row| {
            Ok(AgentSessionEventCursor {
                session_id: session_id.into(),
                generation: row
                    .get_i64("history_generation")
                    .and_then(|n| n.try_into().ok())
                    .ok_or_else(corrupt)?,
                offset: row.get_i64("worker_offset").ok_or_else(corrupt)?,
            })
        })
        .transpose()
}

pub(super) async fn project(
    store: &DatabaseSessionStore,
    params: WorkerProjectionParams,
) -> TalosResult<WorkerProjectionCommit> {
    validate(&params)?;
    if params.page.events.is_empty()
        && !params.page.has_more
        && params.after_cursor.as_ref() == Some(&params.page.next_cursor)
    {
        // An unchanged source still receives identical ownership/generation
        // checks, but local replay must not wait on a concurrent Sync writer.
        let mut snapshot = store.storage.begin_read_transaction().await?;
        let (generation, current) = projection_state(snapshot.as_mut(), &params).await?;
        let active_turn = load_active_turn(snapshot.as_mut(), &params.session_id).await?;
        snapshot.commit().await?;
        return Ok(WorkerProjectionCommit {
            events: Vec::new(),
            active_turn,
            cursor: AgentSessionEventCursor {
                session_id: params.session_id,
                generation,
                offset: current,
            },
        });
    }
    let mut tx = store.begin_session_transaction(&params.session_id).await?;
    let (generation, current) = projection_state(tx.as_mut(), &params).await?;
    let mut output = Vec::new();
    for fact in &params.page.events {
        let fingerprint = format!("{:x}", Sha256::digest(encode(fact)?.as_bytes()));
        let previous = tx.query("SELECT event_fingerprint FROM worker_event_projection WHERE session_id=? AND history_generation=? AND worker_offset=?", &[&params.session_id, &generation.to_string(), &fact.cursor.offset.to_string()]).await?;
        if let Some(row) = previous.first() {
            if row.get_string("event_fingerprint").as_deref() != Some(&fingerprint) {
                return Err(conflict("An already committed Worker event changed"));
            }
            continue;
        }
        if fact.cursor.offset <= current {
            return Err(conflict(
                "Worker source mapping has a gap before the product cursor",
            ));
        }
        let event = project_event(tx.as_mut(), fact).await?;
        metadata::project(tx.as_mut(), fact, &event).await?;
        let settlement = fact.settlement.as_ref().map(encode).transpose()?;
        let command = fact.command.as_ref().map(encode).transpose()?;
        let execution_model = fact.execution_model.as_ref().map(encode).transpose()?;
        tx.execute("INSERT INTO worker_event_projection(session_id,history_generation,worker_offset,product_event_seq,event_fingerprint,settlement_json,command_json,execution_model_json,execution_purpose) VALUES (?,?,?,?,?,?,?,?,?)", &[&params.session_id, &generation.to_string(), &fact.cursor.offset.to_string(), &event.event_seq.to_string(), &fingerprint, settlement.as_deref().unwrap_or("null"), command.as_deref().unwrap_or("null"), execution_model.as_deref().unwrap_or("null"), &encode(&fact.execution_purpose)?]).await?;
        output.push(event);
    }
    let offset = current.max(params.page.next_cursor.offset);
    tx.execute("INSERT INTO worker_projection_cursors(session_id,execution_node_id,history_generation,worker_offset) VALUES (?,?,?,?) ON CONFLICT(session_id) DO UPDATE SET worker_offset=excluded.worker_offset", &[&params.session_id,&params.execution_node_id,&generation.to_string(),&offset.to_string()]).await?;
    let active_turn = load_active_turn(tx.as_mut(), &params.session_id).await?;
    tx.commit().await?;
    Ok(WorkerProjectionCommit {
        events: output,
        active_turn,
        cursor: AgentSessionEventCursor {
            session_id: params.session_id,
            generation,
            offset,
        },
    })
}

async fn projection_state(
    tx: &mut dyn StorageTransaction,
    params: &WorkerProjectionParams,
) -> TalosResult<(u64, i64)> {
    let sessions = tx
        .query(
            "SELECT execution_node_id, history_generation FROM sessions WHERE session_key=?",
            &[&params.session_id],
        )
        .await?;
    let session = sessions
        .first()
        .ok_or_else(|| TalosError::new(ErrorCode::ResourceNotFound, "Session is unavailable"))?;
    if session.get_string("execution_node_id").as_deref() != Some(&params.execution_node_id) {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Worker events do not belong to this Session binding",
        ));
    }
    let generation = session
        .get_i64("history_generation")
        .and_then(|n| u64::try_from(n).ok())
        .ok_or_else(corrupt)?;
    if generation != params.page.next_cursor.generation {
        return Err(TalosError::new(
            ErrorCode::EventCursorExpired,
            "Worker events belong to another Session history generation",
        ));
    }
    let rows = tx.query("SELECT execution_node_id, history_generation, worker_offset FROM worker_projection_cursors WHERE session_id=?", &[&params.session_id]).await?;
    let current = match rows.first() {
        Some(row) => {
            if row.get_string("execution_node_id").as_deref() != Some(&params.execution_node_id)
                || row.get_i64("history_generation") != Some(generation as i64)
            {
                return Err(conflict(
                    "Product source cursor does not match the Worker binding",
                ));
            }
            row.get_i64("worker_offset").ok_or_else(corrupt)?
        }
        None => 0,
    };
    let requested = params.after_cursor.as_ref().map_or(0, |c| c.offset);
    if requested > current {
        return Err(conflict(
            "Worker event page starts beyond the committed product cursor",
        ));
    }
    Ok((generation, current))
}

async fn project_event(
    tx: &mut dyn StorageTransaction,
    fact: &AgentCommittedEvent,
) -> TalosResult<AgentEventRecord> {
    let event = &fact.event;
    let rows = tx.query("SELECT event_seq,session_id,turn_id,sequence,occurred_at,persisted_at,payload_json FROM visible_agent_events WHERE session_id=? AND turn_id=? AND sequence=?", &[&event.identity.session_id,&event.identity.turn_id,&event.sequence.to_string()]).await?;
    if let Some(row) = rows.first() {
        let previous = parse_event_record(row)?;
        if previous.payload != event.payload || previous.occurred_at != event.occurred_at {
            return Err(conflict(
                "Product event differs from its authoritative Worker fact",
            ));
        }
        return Ok(previous);
    }
    let rows = tx.query("SELECT MAX(sequence) AS tail FROM visible_agent_events WHERE session_id=? AND turn_id=?", &[&event.identity.session_id,&event.identity.turn_id]).await?;
    let tail = rows
        .first()
        .and_then(|row| row.get_i64("tail"))
        .unwrap_or(0);
    if event.sequence != (tail as u64).saturating_add(1) {
        return Err(conflict("Worker Turn event sequence has a gap"));
    }
    let active = load_active_turn(tx, &event.identity.session_id).await?;
    if tail == 0 {
        if !matches!(event.payload, AgentEventPayload::TurnStarted(_)) || active.is_some() {
            return Err(conflict(
                "A Worker Turn must start after the previous Turn settles",
            ));
        }
    } else if active
        .as_ref()
        .is_none_or(|active| active.identity != event.identity)
    {
        return Err(conflict("Worker event does not match the active Turn"));
    }
    let mut appended = append_events(tx, vec![event.clone()]).await?;
    let record = appended.pop().ok_or_else(corrupt)?;
    if event.payload.is_terminal() {
        if let (
            AgentEventPayload::TurnCompleted(_),
            Some(AgentExecutionSettlement::Finished { metrics, .. }),
        ) = (&event.payload, &fact.settlement)
        {
            if fact.execution_purpose == agent_protocol::AgentExecutionPurpose::Compaction {
                update_compacted_session(tx, &event.identity.session_id, metrics.clone()).await?;
            } else if active
                .as_ref()
                .is_some_and(|turn| turn.initial_message_id.is_some())
            {
                update_completed_turn_session(tx, &event.identity.session_id, metrics.clone())
                    .await?;
            }
        }
        tx.execute(
            "DELETE FROM agent_continuation_bindings WHERE session_id=? AND turn_id=?",
            &[&event.identity.session_id, &event.identity.turn_id],
        )
        .await?;
        tx.execute(
            "DELETE FROM agent_active_turns WHERE session_id=? AND turn_id=?",
            &[&event.identity.session_id, &event.identity.turn_id],
        )
        .await?;
    } else {
        let state = if matches!(event.payload, AgentEventPayload::WaitingForInput(_)) {
            "waiting_for_input"
        } else {
            "running"
        };
        let message_id = match &event.payload {
            AgentEventPayload::MessageCompleted(message)
                if message.role == AgentMessageRole::User =>
            {
                message.message_id.as_str()
            }
            _ => "",
        };
        tx.execute("INSERT INTO agent_active_turns(session_id,turn_id,initial_message_id,state,last_event_seq,updated_at) VALUES (?,?,NULLIF(?,''),?,?,?) ON CONFLICT(session_id) DO UPDATE SET state=excluded.state,last_event_seq=excluded.last_event_seq,updated_at=excluded.updated_at,initial_message_id=COALESCE(agent_active_turns.initial_message_id,excluded.initial_message_id)", &[&event.identity.session_id,&event.identity.turn_id,message_id,state,&record.event_seq.to_string(),&record.persisted_at.to_string()]).await?;
    }
    Ok(record)
}

fn validate(params: &WorkerProjectionParams) -> TalosResult<()> {
    let last = &params.page.next_cursor;
    if last.session_id != params.session_id
        || last.offset < 0
        || params.page.events.len() > agent_protocol::MAX_EXECUTION_PAGE_EVENTS
    {
        return Err(conflict("Worker event page has an invalid source position"));
    }
    let mut offset = if let Some(after) = &params.after_cursor {
        if after.session_id != params.session_id
            || after.generation != last.generation
            || after.offset < 0
        {
            return Err(conflict(
                "Worker event page has a mismatched starting cursor",
            ));
        }
        after.offset
    } else {
        0
    };
    let mut bytes = 0usize;
    for fact in &params.page.events {
        let size = encode(fact)?.len();
        bytes = bytes.saturating_add(size);
        if size > agent_protocol::MAX_EXECUTION_EVENT_BYTES
            || bytes > agent_protocol::MAX_EXECUTION_PAGE_BYTES
            || fact.cursor.session_id != params.session_id
            || fact.event.identity.session_id != params.session_id
            || fact.cursor.generation != last.generation
            || fact.cursor.offset <= offset
            || fact.event.sequence == 0
        {
            return Err(conflict(
                "Worker event page is oversized or not in source order",
            ));
        }
        match (&fact.event.payload, &fact.settlement) {
            (
                AgentEventPayload::WaitingForInput(_),
                Some(AgentExecutionSettlement::WaitingForInput { .. }),
            ) => {}
            (
                AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_),
                Some(AgentExecutionSettlement::Finished { .. }),
            ) => {}
            (
                AgentEventPayload::WaitingForInput(_)
                | AgentEventPayload::TurnCompleted(_)
                | AgentEventPayload::TurnFailed(_),
                _,
            ) => {
                return Err(conflict(
                    "Worker settlement metadata is missing or inconsistent",
                ))
            }
            (_, Some(_)) => return Err(conflict("Worker progress cannot carry a settlement")),
            (_, None) => {}
        }
        offset = fact.cursor.offset;
    }
    if offset != last.offset || (params.page.events.is_empty() && params.page.has_more) {
        return Err(conflict("Worker page cursor skips facts"));
    }
    Ok(())
}

pub(super) async fn pending(
    store: &DatabaseSessionStore,
    session_id: &str,
) -> TalosResult<Vec<WorkerSettlementRecord>> {
    let rows = store.storage.query("SELECT p.history_generation,p.worker_offset,p.settlement_json,p.execution_purpose,e.event_seq,e.session_id,e.turn_id,e.sequence,e.occurred_at,e.persisted_at,e.payload_json, (SELECT first.command_json FROM worker_event_projection first JOIN visible_agent_events input ON input.event_seq=first.product_event_seq WHERE first.session_id=p.session_id AND first.history_generation=p.history_generation AND input.turn_id=e.turn_id AND first.command_json <> 'null' ORDER BY first.worker_offset LIMIT 1) AS original_command, (SELECT first.execution_model_json FROM worker_event_projection first JOIN visible_agent_events input ON input.event_seq=first.product_event_seq WHERE first.session_id=p.session_id AND first.history_generation=p.history_generation AND input.turn_id=e.turn_id AND first.execution_model_json <> 'null' ORDER BY first.worker_offset LIMIT 1) AS original_model FROM worker_event_projection p JOIN visible_agent_events e ON e.event_seq=p.product_event_seq WHERE p.session_id=? AND p.history_generation=(SELECT history_generation FROM sessions WHERE session_key=p.session_id) AND p.settlement_json <> 'null' AND p.settlement_applied_at IS NULL ORDER BY p.worker_offset LIMIT 256", &[session_id]).await?;
    rows.iter()
        .map(|row| {
            Ok(WorkerSettlementRecord {
                execution_purpose: decode(
                    &row.get_string("execution_purpose").ok_or_else(corrupt)?,
                )?,
                cursor: AgentSessionEventCursor {
                    session_id: session_id.into(),
                    generation: row
                        .get_i64("history_generation")
                        .and_then(|n| n.try_into().ok())
                        .ok_or_else(corrupt)?,
                    offset: row.get_i64("worker_offset").ok_or_else(corrupt)?,
                },
                event: parse_event_record(row)?,
                settlement: decode(&row.get_string("settlement_json").ok_or_else(corrupt)?)?,
                command: row
                    .get_string("original_command")
                    .map(|s| decode(&s))
                    .transpose()?,
                execution_model: row
                    .get_string("original_model")
                    .map(|s| decode(&s))
                    .transpose()?,
            })
        })
        .collect()
}

/// Destructive Session writes cannot remove a receipt before its business owner settles it.
pub(super) async fn ensure_settled(
    tx: &mut dyn StorageTransaction,
    session_ids: &[&str],
) -> TalosResult<()> {
    for ids in session_ids.chunks(256) {
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let rows = tx.query(
            &format!("SELECT 1 FROM worker_event_projection WHERE session_id IN ({placeholders}) AND settlement_json <> 'null' AND settlement_applied_at IS NULL LIMIT 1"),
            ids,
        ).await?;
        if !rows.is_empty() {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Session product settlement must finish before clearing or deleting its history.",
            ));
        }
    }
    Ok(())
}

fn encode<T: serde::Serialize>(value: &T) -> TalosResult<String> {
    serde_json::to_string(value).map_err(|error| {
        TalosError::internal_with_source("Encode Worker product projection.", error)
    })
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> TalosResult<T> {
    serde_json::from_str(value).map_err(|error| {
        TalosError::internal_with_source("Decode Worker product projection.", error)
    })
}
fn corrupt() -> TalosError {
    TalosError::internal("Worker product projection is invalid")
}
fn conflict(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::Conflict, message)
}

#[cfg(test)]
#[path = "worker_projection_tests.rs"]
mod tests;
