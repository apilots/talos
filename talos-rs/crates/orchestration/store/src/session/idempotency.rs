use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_storage::Row;

use super::DatabaseSessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyStatus {
    Pending,
    Accepted,
    Failed,
    FailedNoFirstEvent,
}

impl IdempotencyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Failed => "failed",
            Self::FailedNoFirstEvent => "failed_no_first_event",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageCommandResult {
    pub request_id: String,
    pub message_id: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub status: IdempotencyStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdempotencyRecord {
    pub session_id: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub turn_id: String,
    pub message_id: Option<String>,
    pub status: IdempotencyStatus,
    pub result: Option<MessageCommandResult>,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IdempotencyReservation {
    Reserved(IdempotencyRecord),
    Existing(IdempotencyRecord),
}

#[async_trait]
pub trait MessageIdempotencyStore: Send + Sync {
    async fn get(
        &self,
        session_id: &str,
        idempotency_key: &str,
    ) -> TalosResult<Option<IdempotencyRecord>>;

    async fn reserve(
        &self,
        session_id: &str,
        idempotency_key: &str,
        request_hash: &str,
        turn_id: &str,
        now_ms: i64,
        expires_at_ms: Option<i64>,
    ) -> TalosResult<IdempotencyReservation>;

    async fn complete(
        &self,
        session_id: &str,
        idempotency_key: &str,
        status: IdempotencyStatus,
        message_id: Option<&str>,
        result: &MessageCommandResult,
        now_ms: i64,
    ) -> TalosResult<()>;

    async fn mark_stale_pending_failed(&self, older_than_ms: i64, limit: usize)
        -> TalosResult<u64>;
}

fn parse_status(raw: &str) -> TalosResult<IdempotencyStatus> {
    match raw {
        "pending" => Ok(IdempotencyStatus::Pending),
        "accepted" => Ok(IdempotencyStatus::Accepted),
        "failed" => Ok(IdempotencyStatus::Failed),
        "failed_no_first_event" => Ok(IdempotencyStatus::FailedNoFirstEvent),
        _ => Err(TalosError::SerializationError(format!(
            "unknown idempotency status: {raw}"
        ))),
    }
}

fn parse_record(row: &Row) -> TalosResult<IdempotencyRecord> {
    let result = row
        .get_string("result_json")
        .filter(|value| !value.trim().is_empty())
        .map(|raw| {
            serde_json::from_str::<MessageCommandResult>(&raw).map_err(|err| {
                TalosError::SerializationError(format!("parse idempotency result: {}", err))
            })
        })
        .transpose()?;
    Ok(IdempotencyRecord {
        session_id: row.get_string("session_id").unwrap_or_default(),
        idempotency_key: row.get_string("idempotency_key").unwrap_or_default(),
        request_hash: row.get_string("request_hash").unwrap_or_default(),
        turn_id: row.get_string("turn_id").unwrap_or_default(),
        message_id: row
            .get_string("message_id")
            .filter(|value| !value.trim().is_empty()),
        status: parse_status(row.get_string("status").unwrap_or_default().as_str())?,
        result,
        created_at: row.get_i64("created_at").unwrap_or_default(),
        updated_at: row.get_i64("updated_at").unwrap_or_default(),
        expires_at: row.get_i64("expires_at").filter(|value| *value > 0),
    })
}

impl DatabaseSessionStore {
    async fn get_idempotency_record(
        &self,
        session_id: &str,
        idempotency_key: &str,
    ) -> TalosResult<Option<IdempotencyRecord>> {
        let rows = self
            .storage
            .query(
                r#"
                SELECT session_id, idempotency_key, request_hash, turn_id, message_id, status,
                       result_json, created_at, updated_at, expires_at
                FROM message_idempotency_keys
                WHERE session_id = ? AND idempotency_key = ?
                "#,
                &[session_id, idempotency_key],
            )
            .await?;
        rows.first().map(parse_record).transpose()
    }
}

#[async_trait]
impl MessageIdempotencyStore for DatabaseSessionStore {
    async fn get(
        &self,
        session_id: &str,
        idempotency_key: &str,
    ) -> TalosResult<Option<IdempotencyRecord>> {
        self.get_idempotency_record(session_id, idempotency_key)
            .await
    }

    async fn reserve(
        &self,
        session_id: &str,
        idempotency_key: &str,
        request_hash: &str,
        turn_id: &str,
        now_ms: i64,
        expires_at_ms: Option<i64>,
    ) -> TalosResult<IdempotencyReservation> {
        let expires_cutoff = now_ms.to_string();
        self.storage
            .execute(
                r#"
                DELETE FROM message_idempotency_keys
                WHERE session_id = ? AND idempotency_key = ?
                  AND expires_at > 0 AND expires_at <= ?
                "#,
                &[session_id, idempotency_key, expires_cutoff.as_str()],
            )
            .await?;
        let now = now_ms.to_string();
        let expires = expires_at_ms.unwrap_or_default().to_string();
        let affected = self
            .storage
            .execute(
                r#"
                INSERT OR IGNORE INTO message_idempotency_keys (
                    session_id, idempotency_key, request_hash, turn_id, status,
                    created_at, updated_at, expires_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                &[
                    session_id,
                    idempotency_key,
                    request_hash,
                    turn_id,
                    IdempotencyStatus::Pending.as_str(),
                    now.as_str(),
                    now.as_str(),
                    expires.as_str(),
                ],
            )
            .await?;
        let record = self
            .get_idempotency_record(session_id, idempotency_key)
            .await?
            .ok_or_else(|| {
                TalosError::StorageError("idempotency reserve did not create a row".to_string())
            })?;
        if affected > 0 {
            Ok(IdempotencyReservation::Reserved(record))
        } else {
            Ok(IdempotencyReservation::Existing(record))
        }
    }

    async fn complete(
        &self,
        session_id: &str,
        idempotency_key: &str,
        status: IdempotencyStatus,
        message_id: Option<&str>,
        result: &MessageCommandResult,
        now_ms: i64,
    ) -> TalosResult<()> {
        let result_json = serde_json::to_string(result)
            .map_err(|err| TalosError::SerializationError(err.to_string()))?;
        let now = now_ms.to_string();
        let message_id = message_id.unwrap_or_default();
        self.storage
            .execute(
                r#"
                UPDATE message_idempotency_keys
                SET status = ?, message_id = ?, result_json = ?, updated_at = ?
                WHERE session_id = ? AND idempotency_key = ?
                "#,
                &[
                    status.as_str(),
                    message_id,
                    result_json.as_str(),
                    now.as_str(),
                    session_id,
                    idempotency_key,
                ],
            )
            .await?;
        Ok(())
    }

    async fn mark_stale_pending_failed(
        &self,
        older_than_ms: i64,
        limit: usize,
    ) -> TalosResult<u64> {
        let older_than = older_than_ms.to_string();
        let limit = limit.max(1).to_string();
        let now = chrono::Utc::now().timestamp_millis().to_string();
        self.storage
            .execute(
                r#"
                UPDATE message_idempotency_keys
                SET status = ?, updated_at = ?
                WHERE rowid IN (
                    SELECT rowid
                    FROM message_idempotency_keys
                    WHERE status = ? AND updated_at < ?
                    LIMIT ?
                )
                "#,
                &[
                    IdempotencyStatus::FailedNoFirstEvent.as_str(),
                    now.as_str(),
                    IdempotencyStatus::Pending.as_str(),
                    older_than.as_str(),
                    limit.as_str(),
                ],
            )
            .await
    }
}

#[cfg(test)]
#[path = "idempotency_tests.rs"]
mod tests;
