//! Product notices share the append-only log, not Agent lifecycle ownership.

use talos_agent_api::{AgentEvent, AgentTurnRef, TimelineEventPayload, TimelineSystemNotice};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::Row;

use super::{record::parse_event_record, TimelineEventRecord};
use crate::session::DatabaseSessionStore;

/// Load one bounded batch without per-Turn round trips or unbounded SQL parameters.
pub(super) async fn list(
    store: &DatabaseSessionStore,
    identities: &[AgentTurnRef],
    through_event_seq: i64,
) -> TalosResult<Vec<TimelineEventRecord>> {
    const MAX_TURNS: usize = 1_000;
    const MAX_NOTICES_PER_TURN: usize = 1_024;
    if identities.is_empty() {
        return Ok(Vec::new());
    }
    if identities.len() > MAX_TURNS {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "A notice batch cannot exceed 1,000 Turns.",
        ));
    }
    let requested = serde_json::to_string(identities).map_err(|error| {
        TalosError::internal_with_source("Could not encode notice query identities.", error)
    })?;
    let limit = (MAX_NOTICES_PER_TURN + 1).to_string();
    let rows = store
        .storage
        .query(
            r#"SELECT * FROM (
                SELECT event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json, ROW_NUMBER() OVER (
                    PARTITION BY session_id, turn_id ORDER BY event_seq
                ) AS notice_position
                FROM visible_agent_events
                WHERE (session_id, turn_id) IN (
                    SELECT json_extract(value, '$.sessionId'), json_extract(value, '$.turnId')
                    FROM json_each(?)
                ) AND sequence IS NULL AND event_seq <= ?
            ) WHERE notice_position <= ? ORDER BY event_seq ASC"#,
            &[&requested, &through_event_seq.to_string(), &limit],
        )
        .await?;
    // SQL returns at most one overflow marker per Turn, even for corrupt oversized history.
    if rows.iter().any(|row| {
        row.get_i64("notice_position")
            .is_some_and(|position| position > MAX_NOTICES_PER_TURN as i64)
    }) {
        return Err(TalosError::internal(
            "The Turn notice history exceeds the supported limit.",
        ));
    }
    rows.iter().map(parse).collect()
}

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
    let mut transaction = store
        .begin_session_transaction(&notice.identity.session_id)
        .await?;
    let rows = transaction.query(
        "INSERT INTO agent_events (session_id, turn_id, sequence, occurred_at, persisted_at, payload_json, history_generation)
         SELECT ?1, ?2, NULL, ?3, ?4, ?5, COALESCE((SELECT history_generation FROM sessions WHERE session_key = ?1), 0)
         WHERE EXISTS (SELECT 1 FROM visible_agent_events WHERE session_id = ?1 AND turn_id = ?2 AND sequence IS NOT NULL)
         RETURNING event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json",
        &[&notice.identity.session_id, &notice.identity.turn_id, &occurred_at, &persisted_at, &raw],
    ).await?;
    let result = rows
        .first()
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::ResourceNotFound,
                "The notice target Turn no longer exists.",
            )
        })
        .and_then(parse)?;
    transaction.commit().await?;
    Ok(result)
}

/// The existing receipt is the idempotency key; no separate notice registry is needed.
pub(super) async fn complete_worker_settlement(
    store: &DatabaseSessionStore,
    cursor: &agent_protocol::AgentSessionEventCursor,
    notice: TimelineSystemNotice,
) -> TalosResult<Option<TimelineEventRecord>> {
    if cursor.session_id != notice.identity.session_id {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "The notice belongs to another Session.",
        ));
    }
    let mut transaction = store.begin_session_transaction(&cursor.session_id).await?;
    let receipt = transaction.query(
        "SELECT e.turn_id FROM worker_event_projection p
         JOIN visible_agent_events e ON e.event_seq=p.product_event_seq
         JOIN sessions s ON s.session_key=p.session_id AND s.history_generation=p.history_generation
         WHERE p.session_id=? AND p.history_generation=? AND p.worker_offset=? AND p.settlement_json <> 'null'",
        &[&cursor.session_id, &cursor.generation.to_string(), &cursor.offset.to_string()],
    ).await?;
    let receipt = receipt.first().ok_or_else(|| {
        TalosError::new(
            ErrorCode::ResourceNotFound,
            "The settlement receipt is unavailable.",
        )
    })?;
    if receipt.get_string("turn_id").as_deref() != Some(&notice.identity.turn_id) {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "The notice belongs to another Turn.",
        ));
    }
    let now = chrono::Utc::now().timestamp().to_string();
    let claimed = transaction.query(
        "UPDATE worker_event_projection SET settlement_applied_at=?1
         WHERE session_id=?2 AND history_generation=?3 AND worker_offset=?4
           AND settlement_json <> 'null' AND settlement_applied_at IS NULL
           AND history_generation=(SELECT history_generation FROM sessions WHERE session_key=?2)
           AND EXISTS (SELECT 1 FROM visible_agent_events e WHERE e.event_seq=product_event_seq AND e.session_id=?2 AND e.turn_id=?5)
         RETURNING product_event_seq",
        &[&now, &cursor.session_id, &cursor.generation.to_string(), &cursor.offset.to_string(), &notice.identity.turn_id],
    ).await?;
    if claimed.is_empty() {
        transaction.commit().await?;
        return Ok(None);
    }
    let raw = serde_json::to_string(&notice.notice).map_err(|error| {
        TalosError::internal_with_source("Could not encode the settlement notice.", error)
    })?;
    let rows = transaction.query(
        "INSERT INTO agent_events(session_id,turn_id,sequence,occurred_at,persisted_at,payload_json,history_generation)
         VALUES(?,?,NULL,?,?,?,?) RETURNING event_seq,session_id,turn_id,sequence,occurred_at,persisted_at,payload_json",
        &[&notice.identity.session_id, &notice.identity.turn_id, &notice.occurred_at.to_string(), &now, &raw, &cursor.generation.to_string()],
    ).await?;
    let record = rows
        .first()
        .ok_or_else(|| TalosError::internal("The settlement notice was not committed."))
        .and_then(parse)?;
    transaction.commit().await?;
    Ok(Some(record))
}

pub(crate) fn parse(row: &Row) -> TalosResult<TimelineEventRecord> {
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
