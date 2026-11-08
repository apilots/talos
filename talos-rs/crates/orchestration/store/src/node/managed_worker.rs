//! Durable lifecycle intent and checkpoints in the authoritative Computer directory.

use sqlx::{FromRow, SqliteConnection};
use talos_error::{ApiError, ErrorLocalization, TalosError, TalosResult};
use talos_storage::database_error;

#[derive(Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum ManagedWorkerDesired {
    Running,
    IdleSleep,
    ManualStop,
    Deleting,
}

#[derive(Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum ManagedWorkerAction {
    Create,
    Start,
    Wake,
    Sleep,
    Stop,
    Reset,
    Delete,
}

#[derive(Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
pub enum ManagedWorkerStep {
    Provision,
    Ensure,
    Start,
    Wait,
    Stop,
    RemoveForReset,
    RemoveForDelete,
    RemoveHome,
    RecoverCredential,
    FinalizeDelete,
    Stable,
}

#[derive(Clone, FromRow)]
pub struct ManagedWorkerRecord {
    pub instance_id: String,
    pub owner_user_id: String,
    pub node_id: String,
    pub conversation_session_id: String,
    pub short_id: String,
    pub image: String,
    pub revision: i64,
    pub generation: u32,
    pub desired: ManagedWorkerDesired,
    pub operation: Option<ManagedWorkerAction>,
    pub step: ManagedWorkerStep,
    pub home_ready: bool,
    pub last_activity_at: i64,
    pub operation_at: i64,
    pub error: Option<sqlx::types::Json<ApiError>>,
}

#[derive(FromRow)]
pub struct ManagedWorkerDeletion {
    pub instance_id: String,
    pub node_id: String,
    pub generation: u32,
}

pub enum ManagedWorkerWrite {
    Checkpoint,
    /// Atomically fence new durable admissions and revoke registration.
    Suspend,
}

pub(super) async fn ensure_node_has_no_active_turn(
    tx: &mut SqliteConnection,
    node_id: &str,
) -> TalosResult<()> {
    let active: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sessions s WHERE s.execution_node_id = ? AND (
            EXISTS(SELECT 1 FROM agent_active_turns a WHERE a.session_id = s.session_key)
            OR (SELECT json_extract(e.payload_json, '$.type') FROM agent_events e
                WHERE e.session_id = s.session_key AND e.sequence IS NOT NULL
                ORDER BY e.event_seq DESC LIMIT 1) NOT IN ('turnCompleted', 'turnFailed')))",
    )
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| database_error("check Computer active Turns", error))?;
    if active != 0 {
        return Err(TalosError::localized(
            ErrorLocalization::ComputerDeletionActive,
        ));
    }
    Ok(())
}
