use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_storage::Row;

use super::events::TimelineEventId;
use super::DatabaseSessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveTurnStatus {
    Running,
    WaitingUserAction,
    Cancelling,
    Completed,
    Failed,
}

impl ActiveTurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::WaitingUserAction => "waiting_user_action",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveTurnRecord {
    pub session_id: String,
    pub turn_id: String,
    pub message_id: Option<String>,
    pub status: ActiveTurnStatus,
    pub last_event_id: Option<TimelineEventId>,
    pub current_action_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ActiveTurnUpdate {
    pub session_id: String,
    pub turn_id: String,
    pub message_id: Option<String>,
    pub status: ActiveTurnStatus,
    pub last_event_id: Option<TimelineEventId>,
    pub current_action_id: Option<String>,
    pub updated_at: i64,
}

#[async_trait]
pub trait ActiveTurnStore: Send + Sync {
    async fn upsert_active_turn(&self, update: ActiveTurnUpdate) -> TalosResult<()>;

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<ActiveTurnRecord>>;

    async fn clear_active_turn(&self, session_id: &str) -> TalosResult<()>;
}

fn parse_status(raw: &str) -> TalosResult<ActiveTurnStatus> {
    match raw {
        "running" => Ok(ActiveTurnStatus::Running),
        "waiting_user_action" => Ok(ActiveTurnStatus::WaitingUserAction),
        "cancelling" => Ok(ActiveTurnStatus::Cancelling),
        "completed" => Ok(ActiveTurnStatus::Completed),
        "failed" => Ok(ActiveTurnStatus::Failed),
        _ => Err(TalosError::internal(format!(
            "unknown active turn status: {raw}"
        ))),
    }
}

fn current_action_id(
    status: ActiveTurnStatus,
    current_action_id: Option<String>,
) -> TalosResult<Option<String>> {
    match status {
        ActiveTurnStatus::WaitingUserAction => current_action_id
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| TalosError::internal("waiting_user_action requires current_action_id"))
            .map(Some),
        ActiveTurnStatus::Running
        | ActiveTurnStatus::Cancelling
        | ActiveTurnStatus::Completed
        | ActiveTurnStatus::Failed => Ok(None),
    }
}

fn parse_active_turn_row(row: &Row) -> TalosResult<ActiveTurnRecord> {
    let status = parse_status(row.get_string("status").unwrap_or_default().as_str())?;
    let current_action_id = row
        .get_string("current_action_id")
        .filter(|value| !value.trim().is_empty());
    let last_event_id = row
        .get_i64("last_event_seq")
        .filter(|value| *value > 0)
        .map(TimelineEventId::from_seq);
    Ok(ActiveTurnRecord {
        session_id: row.get_string("session_id").unwrap_or_default(),
        turn_id: row.get_string("turn_id").unwrap_or_default(),
        message_id: row
            .get_string("message_id")
            .filter(|value| !value.trim().is_empty()),
        status,
        last_event_id,
        current_action_id,
        updated_at: row.get_i64("updated_at").unwrap_or_default(),
    })
}

#[async_trait]
impl ActiveTurnStore for DatabaseSessionStore {
    async fn upsert_active_turn(&self, update: ActiveTurnUpdate) -> TalosResult<()> {
        let current_action_id = current_action_id(update.status, update.current_action_id)?;
        let last_event_seq = update
            .last_event_id
            .as_ref()
            .map(TimelineEventId::event_seq)
            .transpose()?
            .unwrap_or_default()
            .to_string();
        let updated_at = update.updated_at.to_string();
        let message_id = update.message_id.unwrap_or_default();
        let current_action_id = current_action_id.unwrap_or_default();
        self.storage
            .execute(
                r#"
                INSERT INTO active_turns (
                    session_id, turn_id, message_id, status, last_event_seq, current_action_id, updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(session_id) DO UPDATE SET
                    turn_id = excluded.turn_id,
                    message_id = excluded.message_id,
                    status = excluded.status,
                    last_event_seq = excluded.last_event_seq,
                    current_action_id = excluded.current_action_id,
                    updated_at = excluded.updated_at
                "#,
                &[
                    update.session_id.as_str(),
                    update.turn_id.as_str(),
                    message_id.as_str(),
                    update.status.as_str(),
                    last_event_seq.as_str(),
                    current_action_id.as_str(),
                    updated_at.as_str(),
                ],
            )
            .await?;
        Ok(())
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<ActiveTurnRecord>> {
        let rows = self
            .storage
            .query(
                "SELECT session_id, turn_id, message_id, status, last_event_seq, current_action_id, updated_at FROM active_turns WHERE session_id = ?",
                &[session_id],
            )
            .await?;
        rows.first().map(parse_active_turn_row).transpose()
    }

    async fn clear_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.storage
            .delete("active_turns", "session_id = ?", &[session_id])
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "active_turn_tests.rs"]
mod tests;
