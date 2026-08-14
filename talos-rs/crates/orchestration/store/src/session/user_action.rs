use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_storage::Row;
use talos_types::{PendingUserAction, UserActionResponse, UserActionState};

use super::DatabaseSessionStore;

/// Database record owned by the UserAction Store boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserActionRecord {
    pub session_id: String,
    pub turn_id: String,
    pub action: PendingUserAction,
    pub response: Option<UserActionResponse>,
    pub version: u64,
    pub updated_at: i64,
}

/// Persists UserAction facts and provides identity-bound atomic compare-and-set.
///
/// Implementations do not decide whether a domain state transition is legal.
#[async_trait]
pub trait UserActionStore: Send + Sync {
    async fn create(&self, record: UserActionRecord) -> TalosResult<UserActionRecord>;

    async fn get(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
    ) -> TalosResult<Option<UserActionRecord>>;

    async fn compare_and_set(
        &self,
        expected: &UserActionRecord,
        next: UserActionRecord,
    ) -> TalosResult<UserActionRecord>;
}

fn serialize<T: serde::Serialize>(value: &T, context: &str) -> TalosResult<String> {
    serde_json::to_string(value)
        .map_err(|error| TalosError::SerializationError(format!("{context}: {error}")))
}

fn state_string(state: UserActionState) -> TalosResult<String> {
    let serialized = serialize(&state, "serialize user action state")?;
    Ok(serialized.trim_matches('"').to_string())
}

fn parse_state(raw: &str) -> TalosResult<UserActionState> {
    serde_json::from_str(&format!("\"{raw}\"")).map_err(|error| {
        TalosError::SerializationError(format!("parse user action state: {error}"))
    })
}

fn parse_record(row: &Row) -> TalosResult<UserActionRecord> {
    let action = row
        .get_string("action_json")
        .ok_or_else(|| TalosError::SerializationError("user action missing action_json".into()))
        .and_then(|raw| {
            serde_json::from_str::<PendingUserAction>(&raw).map_err(|error| {
                TalosError::SerializationError(format!("parse user action: {error}"))
            })
        })?;
    let stored_state = parse_state(row.get_string("state").unwrap_or_default().as_str())?;
    if action.state != stored_state {
        return Err(TalosError::SerializationError(
            "user action state does not match action payload".to_string(),
        ));
    }
    let response = row
        .get_string("response_json")
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| {
            serde_json::from_str::<UserActionResponse>(&raw).map_err(|error| {
                TalosError::SerializationError(format!("parse user action response: {error}"))
            })
        })
        .transpose()?;
    let version = row
        .get_i64("version")
        .unwrap_or_default()
        .try_into()
        .map_err(|_| TalosError::SerializationError("negative user action version".to_string()))?;
    Ok(UserActionRecord {
        session_id: row.get_string("session_id").unwrap_or_default(),
        turn_id: row.get_string("turn_id").unwrap_or_default(),
        action,
        response,
        version,
        updated_at: row.get_i64("updated_at").unwrap_or_default(),
    })
}

fn validate_identity(record: &UserActionRecord) -> TalosResult<()> {
    if record.session_id.trim().is_empty()
        || record.turn_id.trim().is_empty()
        || record.action.action_id.trim().is_empty()
    {
        return Err(TalosError::ValidationError(
            "user action identity must not be empty".to_string(),
        ));
    }
    Ok(())
}

#[async_trait]
impl UserActionStore for DatabaseSessionStore {
    async fn create(&self, record: UserActionRecord) -> TalosResult<UserActionRecord> {
        validate_identity(&record)?;
        let action_json = serialize(&record.action, "serialize user action")?;
        let response_json = record
            .response
            .as_ref()
            .map(|response| serialize(response, "serialize user action response"))
            .transpose()?
            .unwrap_or_default();
        let state = state_string(record.action.state)?;
        let version = record.version.to_string();
        let updated_at = record.updated_at.to_string();
        self.storage
            .execute(
                r#"
                INSERT INTO user_actions (
                    session_id, turn_id, action_id, action_json, response_json,
                    state, version, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(session_id, turn_id, action_id) DO NOTHING
                "#,
                &[
                    record.session_id.as_str(),
                    record.turn_id.as_str(),
                    record.action.action_id.as_str(),
                    action_json.as_str(),
                    response_json.as_str(),
                    state.as_str(),
                    version.as_str(),
                    updated_at.as_str(),
                ],
            )
            .await?;
        let stored = self
            .get(
                &record.session_id,
                &record.turn_id,
                &record.action.action_id,
            )
            .await?
            .ok_or_else(|| {
                TalosError::StorageError("user action create returned no record".to_string())
            })?;
        if stored == record {
            return Ok(stored);
        }
        Err(TalosError::Conflict(format!(
            "user action '{}' already exists with different content",
            record.action.action_id
        )))
    }

    async fn get(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
    ) -> TalosResult<Option<UserActionRecord>> {
        let rows = self
            .storage
            .query(
                r#"
                SELECT session_id, turn_id, action_id, action_json, response_json,
                       state, version, updated_at
                FROM user_actions
                WHERE session_id = ? AND turn_id = ? AND action_id = ?
                "#,
                &[session_id, turn_id, action_id],
            )
            .await?;
        rows.first().map(parse_record).transpose()
    }

    async fn compare_and_set(
        &self,
        expected: &UserActionRecord,
        next: UserActionRecord,
    ) -> TalosResult<UserActionRecord> {
        validate_identity(expected)?;
        validate_identity(&next)?;
        if expected.session_id != next.session_id
            || expected.turn_id != next.turn_id
            || expected.action.action_id != next.action.action_id
        {
            return Err(TalosError::ValidationError(
                "user action compare-and-set identity must not change".to_string(),
            ));
        }
        if next.version != expected.version.saturating_add(1) {
            return Err(TalosError::ValidationError(
                "user action compare-and-set must increment version by one".to_string(),
            ));
        }
        let action_json = serialize(&next.action, "serialize user action")?;
        let response_json = next
            .response
            .as_ref()
            .map(|response| serialize(response, "serialize user action response"))
            .transpose()?
            .unwrap_or_default();
        let next_state = state_string(next.action.state)?;
        let next_version = next.version.to_string();
        let updated_at = next.updated_at.to_string();
        let expected_state = state_string(expected.action.state)?;
        let expected_version = expected.version.to_string();
        let affected = self
            .storage
            .execute(
                r#"
                UPDATE user_actions
                SET action_json = ?, response_json = ?, state = ?, version = ?, updated_at = ?
                WHERE session_id = ? AND turn_id = ? AND action_id = ?
                  AND state = ? AND version = ?
                "#,
                &[
                    action_json.as_str(),
                    response_json.as_str(),
                    next_state.as_str(),
                    next_version.as_str(),
                    updated_at.as_str(),
                    next.session_id.as_str(),
                    next.turn_id.as_str(),
                    next.action.action_id.as_str(),
                    expected_state.as_str(),
                    expected_version.as_str(),
                ],
            )
            .await?;
        if affected == 0 {
            return Err(TalosError::Conflict(format!(
                "user action '{}' state or version changed",
                next.action.action_id
            )));
        }
        self.get(&next.session_id, &next.turn_id, &next.action.action_id)
            .await?
            .ok_or_else(|| {
                TalosError::StorageError(
                    "user action compare-and-set returned no record".to_string(),
                )
            })
    }
}

#[cfg(test)]
#[path = "user_action_tests.rs"]
mod tests;
