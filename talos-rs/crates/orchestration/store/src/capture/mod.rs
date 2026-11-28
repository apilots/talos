//! Always-on transactional facts consumed by optional replication adapters.
//! This module owns raw Store cuts, not network, account login or Sync codecs.
#[cfg(feature = "sync")]
mod bindings;
mod metadata;
pub(crate) mod source;
mod transaction;
#[cfg(feature = "sync")]
pub use bindings::StoreCaptureBindings;
#[cfg(not(feature = "sync"))]
#[derive(Default)]
pub struct StoreCaptureBindings;
#[cfg(not(feature = "sync"))]
impl StoreCaptureBindings {
    pub(crate) fn wake_capture(&self) {}
}
pub(crate) use metadata::{MetadataRevision, MetadataSource};
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
pub(crate) use transaction::{finish, prepare, transaction, SqliteCaptureTransaction};
#[derive(Clone)]
pub(crate) enum ResourceKey {
    Project { project_id: String },
    File { file_id: String },
}
impl ResourceKey {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::File { .. } => "file",
        }
    }
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Project { project_id } => project_id,
            Self::File { file_id } => file_id,
        }
    }
}
pub(crate) fn text(row: &Row, key: &str) -> TalosResult<String> {
    row.get_string(key)
        .ok_or_else(|| TalosError::internal("Missing persisted resource field."))
}
pub(crate) fn number(row: &Row, key: &str) -> TalosResult<i64> {
    row.get_i64(key)
        .ok_or_else(|| TalosError::internal("Missing persisted resource field."))
}
async fn event_tail(
    tx: &mut dyn StorageTransaction,
    session: &str,
    generation: u64,
) -> TalosResult<i64> {
    tail(tx, "SELECT MAX(event_seq) AS tail FROM committed_agent_events WHERE session_id=? AND history_generation=?", session, generation).await
}
async fn command_tail(
    tx: &mut dyn StorageTransaction,
    session: &str,
    generation: u64,
) -> TalosResult<i64> {
    tail(tx, "SELECT MAX(revision) AS tail FROM session_command_revisions WHERE session_id=? AND history_generation=?", session, generation).await
}
async fn tail(
    tx: &mut dyn StorageTransaction,
    sql: &str,
    session: &str,
    generation: u64,
) -> TalosResult<i64> {
    Ok(tx
        .query(sql, &[session, &generation.to_string()])
        .await?
        .first()
        .and_then(|row| row.get_i64("tail"))
        .unwrap_or(0))
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
