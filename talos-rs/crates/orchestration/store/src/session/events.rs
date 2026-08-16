use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_storage::Row;
use talos_types::talos::{AgentEvent, AgentEventPayload, AgentEventType};

use super::DatabaseSessionStore;

pub const EVENT_ID_PREFIX: &str = "ev_";
const EVENT_ID_WIDTH: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimelineEventId(String);

impl TimelineEventId {
    pub fn from_seq(event_seq: i64) -> Self {
        Self(format!(
            "{EVENT_ID_PREFIX}{event_seq:0width$}",
            width = EVENT_ID_WIDTH
        ))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn event_seq(&self) -> TalosResult<i64> {
        let raw = self.0.strip_prefix(EVENT_ID_PREFIX).ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("invalid timeline event id: {}", self.0),
            )
        })?;
        raw.parse::<i64>().map_err(|err| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("invalid timeline event id {}: {}", self.0, err),
            )
        })
    }
}

impl std::fmt::Display for TimelineEventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<TimelineEventId> for String {
    fn from(value: TimelineEventId) -> Self {
        value.0
    }
}

impl From<String> for TimelineEventId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[async_trait]
pub trait TimelineEventStore: Send + Sync {
    async fn append_event(&self, event: AgentEvent) -> TalosResult<AgentEvent>;

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>>;

    /// Lists the latest bounded event projection for one exact Turn in event order.
    async fn list_turn_events(
        &self,
        session_id: &str,
        turn_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>>;

    async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>>;

    async fn oldest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>>;

    async fn delete_completed_events_before(
        &self,
        before_timestamp_ms: i64,
        limit: usize,
    ) -> TalosResult<u64>;

    async fn delete_overflow_events_per_session(
        &self,
        max_events_per_session: usize,
        limit: usize,
    ) -> TalosResult<u64>;
}

pub(crate) fn parse_event_row(row: &Row) -> TalosResult<AgentEvent> {
    let event_seq = row
        .get_i64("event_seq")
        .ok_or_else(|| TalosError::internal("timeline event missing event_seq"))?;
    let event_type = row
        .get_string("event_type")
        .ok_or_else(|| TalosError::internal("timeline event missing event_type"))
        .and_then(|raw| {
            serde_json::from_str::<AgentEventType>(&format!("\"{raw}\""))
                .map_err(|err| TalosError::internal(format!("parse agent event type: {}", err)))
        })?;
    let payload = row
        .get_string("payload_json")
        .ok_or_else(|| TalosError::internal("timeline event missing payload"))
        .and_then(|raw| {
            serde_json::from_str::<AgentEventPayload>(&raw)
                .map_err(|err| TalosError::internal(format!("parse agent event payload: {}", err)))
        })?;
    let sequence = row
        .get_i64("sequence")
        .unwrap_or_default()
        .try_into()
        .unwrap_or_default();
    Ok(AgentEvent {
        event_seq: Some(event_seq),
        event_id: Some(TimelineEventId::from_seq(event_seq).to_string()),
        created_at_ms: Some(row.get_i64("created_at_ms").unwrap_or_default()),
        event_type,
        turn_id: row.get_string("turn_id").unwrap_or_default(),
        message_id: row.get_string("message_id").unwrap_or_default(),
        session_id: row.get_string("session_id").unwrap_or_default(),
        sequence,
        payload,
    })
}

#[async_trait]
impl TimelineEventStore for DatabaseSessionStore {
    async fn append_event(&self, event: AgentEvent) -> TalosResult<AgentEvent> {
        let event_type = serde_json::to_value(event.event_type)
            .map_err(|err| TalosError::internal(err.to_string()))?
            .as_str()
            .ok_or_else(|| TalosError::internal("event type is not a string"))?
            .to_string();
        let payload_json = serde_json::to_string(&event.payload)
            .map_err(|err| TalosError::internal(err.to_string()))?;
        let sequence = event.sequence.to_string();
        let created_at_ms = event
            .created_at_ms
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
            .to_string();
        let sql = r#"
            INSERT INTO timeline_events (
                session_id, turn_id, message_id, sequence, event_type, payload_json, created_at_ms
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING event_seq, session_id, turn_id, message_id, sequence, event_type, payload_json, created_at_ms
        "#;
        let rows = self
            .storage
            .query(
                sql,
                &[
                    event.session_id.as_str(),
                    event.turn_id.as_str(),
                    event.message_id.as_str(),
                    sequence.as_str(),
                    event_type.as_str(),
                    payload_json.as_str(),
                    created_at_ms.as_str(),
                ],
            )
            .await?;
        let row = rows.into_iter().next().ok_or_else(|| {
            TalosError::internal("append timeline event returned no row".to_string())
        })?;
        parse_event_row(&row)
    }

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>> {
        let after_seq = match after_event_id {
            Some(event_id) => event_id.event_seq()?,
            None => 0,
        };
        let limit = limit.max(1).to_string();
        let after_seq = after_seq.to_string();
        let rows = self
            .storage
            .query(
                r#"
                SELECT event_seq, session_id, turn_id, message_id, sequence, event_type, payload_json, created_at_ms
                FROM timeline_events
                WHERE session_id = ? AND event_seq > ?
                ORDER BY event_seq ASC
                LIMIT ?
                "#,
                &[session_id, after_seq.as_str(), limit.as_str()],
            )
            .await?;
        rows.iter().map(parse_event_row).collect()
    }

    async fn list_turn_events(
        &self,
        session_id: &str,
        turn_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>> {
        let limit = limit.max(1).to_string();
        let rows = self
            .storage
            .query(
                r#"
                SELECT event_seq, session_id, turn_id, message_id, sequence, event_type, payload_json, created_at_ms
                FROM (
                    SELECT event_seq, session_id, turn_id, message_id, sequence, event_type, payload_json, created_at_ms
                    FROM timeline_events
                    WHERE session_id = ? AND turn_id = ?
                    ORDER BY event_seq DESC
                    LIMIT ?
                )
                ORDER BY event_seq ASC
                "#,
                &[session_id, turn_id, limit.as_str()],
            )
            .await?;
        rows.iter().map(parse_event_row).collect()
    }

    async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        let rows = self
            .storage
            .query(
                "SELECT event_seq FROM timeline_events WHERE session_id = ? ORDER BY event_seq DESC LIMIT 1",
                &[session_id],
            )
            .await?;
        Ok(rows
            .first()
            .and_then(|row| row.get_i64("event_seq"))
            .map(TimelineEventId::from_seq))
    }

    async fn oldest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        let rows = self
            .storage
            .query(
                "SELECT event_seq FROM timeline_events WHERE session_id = ? ORDER BY event_seq ASC LIMIT 1",
                &[session_id],
            )
            .await?;
        Ok(rows
            .first()
            .and_then(|row| row.get_i64("event_seq"))
            .map(TimelineEventId::from_seq))
    }

    async fn delete_completed_events_before(
        &self,
        before_timestamp_ms: i64,
        limit: usize,
    ) -> TalosResult<u64> {
        let before = before_timestamp_ms.to_string();
        let limit = limit.max(1).to_string();
        self.storage
            .execute(
                r#"
                DELETE FROM timeline_events
                WHERE event_seq IN (
                    SELECT event_seq
                    FROM timeline_events
                    WHERE created_at_ms < ?
                      AND session_id NOT IN (SELECT session_id FROM active_turns)
                    ORDER BY event_seq ASC
                    LIMIT ?
                )
                "#,
                &[before.as_str(), limit.as_str()],
            )
            .await
    }

    async fn delete_overflow_events_per_session(
        &self,
        max_events_per_session: usize,
        limit: usize,
    ) -> TalosResult<u64> {
        let max_events = max_events_per_session.max(1).to_string();
        let limit = limit.max(1).to_string();
        self.storage
            .execute(
                r#"
                DELETE FROM timeline_events
                WHERE event_seq IN (
                    SELECT event_seq
                    FROM (
                        SELECT event_seq,
                               session_id,
                               ROW_NUMBER() OVER (
                                   PARTITION BY session_id
                                   ORDER BY event_seq DESC
                               ) AS retained_rank
                        FROM timeline_events
                        WHERE session_id NOT IN (SELECT session_id FROM active_turns)
                    )
                    WHERE retained_rank > CAST(? AS INTEGER)
                    ORDER BY event_seq ASC
                    LIMIT ?
                )
                "#,
                &[max_events.as_str(), limit.as_str()],
            )
            .await
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
