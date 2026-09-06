//! Product notices share the append-only log, not Agent lifecycle ownership.

use talos_agent_api::{AgentEvent, TimelineEventPayload, TimelineSystemNotice};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::Row;

use super::{record::parse_event_record, TimelineEventRecord};
use crate::session::DatabaseSessionStore;

pub(super) async fn append(
    store: &DatabaseSessionStore,
    notice: TimelineSystemNotice,
) -> TalosResult<TimelineEventRecord> {
    let raw = serde_json::to_string(&notice.notice).map_err(|error| {
        TalosError::internal_with_source("Could not encode the session notice.", error)
    })?;
    let persisted_at = chrono::Utc::now().timestamp().to_string();
    let occurred_at = notice.occurred_at.to_string();
    // Existence and insertion share one statement, including concurrent Session deletion.
    let rows = store.storage.query(
        "INSERT INTO agent_events (session_id, turn_id, sequence, occurred_at, persisted_at, payload_json) SELECT ?, ?, NULL, ?, ?, ? WHERE EXISTS (SELECT 1 FROM agent_events WHERE session_id = ? AND turn_id = ? AND sequence IS NOT NULL) RETURNING *",
        &[&notice.identity.session_id, &notice.identity.turn_id, &occurred_at, &persisted_at, &raw, &notice.identity.session_id, &notice.identity.turn_id],
    ).await?;
    rows.first()
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::ResourceNotFound,
                "The notice target Turn no longer exists.",
            )
        })
        .and_then(parse)
}

pub(super) fn parse(row: &Row) -> TalosResult<TimelineEventRecord> {
    if row.get_i64("sequence").is_some() {
        let record = parse_event_record(row)?;
        return Ok(TimelineEventRecord {
            event_seq: record.event_seq,
            persisted_at: record.persisted_at,
            event: TimelineEventPayload::Agent(AgentEvent {
                identity: record.identity,
                sequence: record.sequence,
                occurred_at: record.occurred_at,
                payload: record.payload,
            }),
        });
    }
    let missing = || TalosError::internal("The stored session notice is incomplete.");
    let raw = row.get_string("payload_json").ok_or_else(missing)?;
    Ok(TimelineEventRecord {
        event_seq: row.get_i64("event_seq").ok_or_else(missing)?,
        persisted_at: row.get_i64("persisted_at").ok_or_else(missing)?,
        event: TimelineEventPayload::SystemNotice(TimelineSystemNotice {
            identity: talos_agent_api::AgentTurnRef::new(
                row.get_string("session_id").ok_or_else(missing)?,
                row.get_string("turn_id").ok_or_else(missing)?,
            ),
            occurred_at: row.get_i64("occurred_at").ok_or_else(missing)?,
            notice: serde_json::from_str(&raw).map_err(|error| {
                TalosError::internal_with_source(
                    "Could not decode the stored session notice.",
                    error,
                )
            })?,
        }),
    })
}
