use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_storage::Row;

use crate::events::SessionEventId;
use crate::DatabaseSessionStore;

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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PendingUserAction {
    ToolApproval(PendingToolApproval),
    DynamicToolResult(PendingDynamicToolResult),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingToolApproval {
    pub call_id: String,
    pub tool_name: String,
    pub prompt: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingDynamicToolResult {
    pub call_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveTurnRecord {
    pub session_id: String,
    pub turn_id: String,
    pub message_id: Option<String>,
    pub status: ActiveTurnStatus,
    pub last_event_id: Option<SessionEventId>,
    pub pending_user_action: Option<PendingUserAction>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ActiveTurnUpdate {
    pub session_id: String,
    pub turn_id: String,
    pub message_id: Option<String>,
    pub status: ActiveTurnStatus,
    pub last_event_id: Option<SessionEventId>,
    pub pending_user_action: Option<PendingUserAction>,
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
        _ => Err(TalosError::SerializationError(format!(
            "unknown active turn status: {raw}"
        ))),
    }
}

fn pending_json(
    status: ActiveTurnStatus,
    pending: Option<PendingUserAction>,
) -> TalosResult<Option<String>> {
    match status {
        ActiveTurnStatus::WaitingUserAction => pending
            .map(|value| {
                serde_json::to_string(&value)
                    .map_err(|err| TalosError::SerializationError(err.to_string()))
            })
            .transpose()?
            .ok_or_else(|| {
                TalosError::ValidationError(
                    "waiting_user_action requires pending_user_action".to_string(),
                )
            })
            .map(Some),
        ActiveTurnStatus::Cancelling | ActiveTurnStatus::Completed | ActiveTurnStatus::Failed => {
            Ok(None)
        }
        ActiveTurnStatus::Running => Ok(pending
            .map(|value| {
                serde_json::to_string(&value)
                    .map_err(|err| TalosError::SerializationError(err.to_string()))
            })
            .transpose()?),
    }
}

fn parse_active_turn_row(row: &Row) -> TalosResult<ActiveTurnRecord> {
    let status = parse_status(row.get_string("status").unwrap_or_default().as_str())?;
    let pending_user_action = row
        .get_string("pending_user_action_json")
        .filter(|value| !value.trim().is_empty())
        .map(|raw| {
            serde_json::from_str::<PendingUserAction>(&raw).map_err(|err| {
                TalosError::SerializationError(format!("parse pending user action: {}", err))
            })
        })
        .transpose()?;
    let last_event_id = row
        .get_i64("last_event_seq")
        .filter(|value| *value > 0)
        .map(SessionEventId::from_seq);
    Ok(ActiveTurnRecord {
        session_id: row.get_string("session_id").unwrap_or_default(),
        turn_id: row.get_string("turn_id").unwrap_or_default(),
        message_id: row
            .get_string("message_id")
            .filter(|value| !value.trim().is_empty()),
        status,
        last_event_id,
        pending_user_action,
        updated_at: row.get_i64("updated_at").unwrap_or_default(),
    })
}

#[async_trait]
impl ActiveTurnStore for DatabaseSessionStore {
    async fn upsert_active_turn(&self, update: ActiveTurnUpdate) -> TalosResult<()> {
        let pending = pending_json(update.status, update.pending_user_action)?;
        let last_event_seq = update
            .last_event_id
            .as_ref()
            .map(SessionEventId::event_seq)
            .transpose()?
            .unwrap_or_default()
            .to_string();
        let updated_at = update.updated_at.to_string();
        let message_id = update.message_id.unwrap_or_default();
        let pending = pending.unwrap_or_default();
        self.storage
            .execute(
                r#"
                INSERT INTO active_turns (
                    session_id, turn_id, message_id, status, last_event_seq, pending_user_action_json, updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(session_id) DO UPDATE SET
                    turn_id = excluded.turn_id,
                    message_id = excluded.message_id,
                    status = excluded.status,
                    last_event_seq = excluded.last_event_seq,
                    pending_user_action_json = excluded.pending_user_action_json,
                    updated_at = excluded.updated_at
                "#,
                &[
                    update.session_id.as_str(),
                    update.turn_id.as_str(),
                    message_id.as_str(),
                    update.status.as_str(),
                    last_event_seq.as_str(),
                    pending.as_str(),
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
                "SELECT session_id, turn_id, message_id, status, last_event_seq, pending_user_action_json, updated_at FROM active_turns WHERE session_id = ?",
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
mod tests {
    use super::*;
    use std::sync::Arc;
    use talos_storage::SqliteStorage;

    async fn store() -> DatabaseSessionStore {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        DatabaseSessionStore::new(storage).await.unwrap()
    }

    #[tokio::test]
    async fn cancelling_clears_pending_user_action() {
        let store = store().await;
        store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: "s1".to_string(),
                turn_id: "t1".to_string(),
                message_id: Some("m1".to_string()),
                status: ActiveTurnStatus::Cancelling,
                last_event_id: Some(SessionEventId::from_seq(3)),
                pending_user_action: Some(PendingUserAction::DynamicToolResult(
                    PendingDynamicToolResult {
                        call_id: "c1".to_string(),
                        tool_name: "tool".to_string(),
                    },
                )),
                updated_at: 10,
            })
            .await
            .unwrap();

        let record = store.get_active_turn("s1").await.unwrap().unwrap();
        assert_eq!(record.status, ActiveTurnStatus::Cancelling);
        assert_eq!(record.pending_user_action, None);
    }
}
