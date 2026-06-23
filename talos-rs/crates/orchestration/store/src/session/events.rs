use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_storage::Row;
use talos_types::talos::{TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineEventType};

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
            TalosError::ValidationError(format!("invalid timeline event id: {}", self.0))
        })?;
        raw.parse::<i64>().map_err(|err| {
            TalosError::ValidationError(format!("invalid timeline event id {}: {}", self.0, err))
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
    async fn append_event(&self, event: TalosTimelineEvent) -> TalosResult<TalosTimelineEvent>;

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<TalosTimelineEvent>>;

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

pub(crate) fn parse_event_row(row: &Row) -> TalosResult<TalosTimelineEvent> {
    let event_seq = row
        .get_i64("event_seq")
        .ok_or_else(|| TalosError::SerializationError("timeline event missing event_seq".into()))?;
    let event_type = row
        .get_string("event_type")
        .ok_or_else(|| TalosError::SerializationError("timeline event missing event_type".into()))
        .and_then(|raw| {
            serde_json::from_str::<TalosTimelineEventType>(&format!("\"{raw}\"")).map_err(|err| {
                TalosError::SerializationError(format!("parse timeline event type: {}", err))
            })
        })?;
    let payload = row
        .get_string("payload_json")
        .ok_or_else(|| TalosError::SerializationError("timeline event missing payload".into()))
        .and_then(|raw| {
            serde_json::from_str::<TalosTimelineEventPayload>(&raw).map_err(|err| {
                TalosError::SerializationError(format!("parse timeline event payload: {}", err))
            })
        })?;
    let sequence = row
        .get_i64("sequence")
        .unwrap_or_default()
        .try_into()
        .unwrap_or_default();
    Ok(TalosTimelineEvent {
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
    async fn append_event(&self, event: TalosTimelineEvent) -> TalosResult<TalosTimelineEvent> {
        let event_type = serde_json::to_value(&event.event_type)
            .map_err(|err| TalosError::SerializationError(err.to_string()))?
            .as_str()
            .ok_or_else(|| TalosError::SerializationError("event type is not a string".into()))?
            .to_string();
        let payload_json = serde_json::to_string(&event.payload)
            .map_err(|err| TalosError::SerializationError(err.to_string()))?;
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
            TalosError::StorageError("append timeline event returned no row".to_string())
        })?;
        parse_event_row(&row)
    }

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<TalosTimelineEvent>> {
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
mod tests {
    use super::*;
    use crate::session::active_turn::{ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate};
    use std::sync::Arc;
    use talos_storage::SqliteStorage;
    use talos_types::talos::TalosTurnStartedEvent;

    async fn store() -> DatabaseSessionStore {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        DatabaseSessionStore::new(storage).await.unwrap()
    }

    fn started_event(session_id: &str, sequence: u64) -> TalosTimelineEvent {
        TalosTimelineEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: Some(1000),
            event_type: TalosTimelineEventType::TurnStarted,
            turn_id: "turn_1".to_string(),
            message_id: "msg_1".to_string(),
            session_id: session_id.to_string(),
            sequence,
            payload: TalosTimelineEventPayload::TurnStarted(TalosTurnStartedEvent {
                request_id: "req_1".to_string(),
                turn_id: "turn_1".to_string(),
                started_at_ms: 1000,
            }),
        }
    }

    #[tokio::test]
    async fn append_event_assigns_monotonic_event_id() {
        let store = store().await;
        let first = store.append_event(started_event("s1", 0)).await.unwrap();
        let second = store.append_event(started_event("s1", 1)).await.unwrap();

        assert_eq!(first.event_seq, Some(1));
        assert_eq!(first.event_id.as_deref(), Some("ev_00000000000000000001"));
        assert_eq!(second.event_seq, Some(2));
        assert_eq!(second.event_id.as_deref(), Some("ev_00000000000000000002"));
    }

    #[tokio::test]
    async fn list_events_after_uses_event_cursor() {
        let store = store().await;
        let first = store.append_event(started_event("s1", 0)).await.unwrap();
        store.append_event(started_event("s1", 1)).await.unwrap();
        store.append_event(started_event("s2", 0)).await.unwrap();
        let cursor = TimelineEventId::from(first.event_id.unwrap());

        let events = store
            .list_events_after("s1", Some(&cursor), 10)
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);
    }

    #[tokio::test]
    async fn oldest_event_id_returns_first_retained_event() {
        let store = store().await;
        let first = store.append_event(started_event("s1", 0)).await.unwrap();
        store.append_event(started_event("s1", 1)).await.unwrap();

        let oldest = store.oldest_event_id("s1").await.unwrap();

        assert_eq!(oldest.map(|id| id.to_string()), first.event_id);
    }

    #[tokio::test]
    async fn gc_does_not_delete_active_turn_events() {
        let store = store().await;
        store
            .append_event(started_event("active", 0))
            .await
            .unwrap();
        store.append_event(started_event("done", 0)).await.unwrap();
        store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: "active".to_string(),
                turn_id: "turn_1".to_string(),
                message_id: Some("msg_1".to_string()),
                status: ActiveTurnStatus::Running,
                last_event_id: None,
                pending_user_action: None,
                updated_at: 1000,
            })
            .await
            .unwrap();

        let deleted = store
            .delete_completed_events_before(2000, 10)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        let active_events = store.list_events_after("active", None, 10).await.unwrap();
        let done_events = store.list_events_after("done", None, 10).await.unwrap();
        assert_eq!(active_events.len(), 1);
        assert!(done_events.is_empty());
    }

    #[tokio::test]
    async fn gc_keeps_recent_events_per_completed_session() {
        let store = store().await;
        store.append_event(started_event("done", 0)).await.unwrap();
        store.append_event(started_event("done", 1)).await.unwrap();
        store.append_event(started_event("done", 2)).await.unwrap();

        let deleted = store
            .delete_overflow_events_per_session(2, 10)
            .await
            .unwrap();

        assert_eq!(deleted, 1);
        let remaining = store.list_events_after("done", None, 10).await.unwrap();
        assert_eq!(
            remaining
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }
}
