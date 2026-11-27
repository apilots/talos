use crate::{SyncResourceKey, SyncResourceSummary, SyncScope, SyncVersion};
use serde::{de::DeserializeOwned, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
use talos_sync_parser::{ComputerAlias, SyncBinding};

use talos_sync_parser::codec_error;

mod checkpoint;
pub(crate) use checkpoint::{
    acknowledgement, copy_checkpoint_scope, document, document_checkpoint, document_version,
    resource_summary, save_acknowledgement, save_document,
};

pub(crate) mod snapshot;
pub(crate) mod staged_write;

pub(crate) struct DocumentRecord {
    pub snapshot: Vec<u8>,
    pub schema_version: i64,
    pub version: SyncVersion,
    pub acknowledged: SyncVersion,
    pub has_session_projection: bool,
}

pub(crate) struct AcknowledgementRecord {
    pub exported: SyncVersion,
    pub current: SyncVersion,
    pub acknowledged: SyncVersion,
}

pub(crate) struct OutboxRecord {
    pub seq: i64,
    pub changes: Vec<talos_sync_types::SyncChange>,
    pub exported_version: Option<SyncVersion>,
}

pub(crate) async fn identity(tx: &mut dyn StorageTransaction) -> TalosResult<(String, u64)> {
    let peer = uuid::Uuid::new_v4();
    let writer = u64::from_be_bytes(
        peer.as_bytes()[..8]
            .try_into()
            .expect("UUID prefix is eight bytes"),
    );
    tx.execute(
        "INSERT OR IGNORE INTO sync_identity(singleton, peer_id, loro_writer_id) VALUES (1, ?, ?)",
        &[&peer.to_string(), &writer.to_string()],
    )
    .await?;
    let rows = tx
        .query(
            "SELECT peer_id, loro_writer_id FROM sync_identity WHERE singleton = 1",
            &[],
        )
        .await?;
    let row = rows.first().ok_or_else(corrupt_record)?;
    let writer = text(row, "loro_writer_id")?.parse().map_err(|error| {
        TalosError::internal_with_source(
            "Read synchronization identity; technical details were withheld.",
            error,
        )
    })?;
    Ok((text(row, "peer_id")?, writer))
}

pub(crate) async fn binding(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    user: &str,
    peer: &str,
) -> TalosResult<Option<SyncBinding>> {
    let rows = tx.query("SELECT computers_json FROM sync_bindings WHERE hub_id = ? AND account_id = ? AND source_user_id = ?", &[&scope.hub_id, &scope.account_id, user]).await?;
    rows.first()
        .map(|row| {
            Ok(SyncBinding {
                scope: scope.clone(),
                peer_id: peer.into(),
                source_user_id: user.into(),
                computers: json(row, "computers_json")?,
            })
        })
        .transpose()
}

pub(crate) async fn save_binding(
    tx: &mut dyn StorageTransaction,
    binding: &SyncBinding,
) -> TalosResult<()> {
    let rows = tx.query("SELECT account_id, computers_json FROM sync_bindings WHERE hub_id = ? AND source_user_id = ?", &[&binding.scope.hub_id, &binding.source_user_id]).await?;
    if let Some(row) = rows.first() {
        let old: Vec<ComputerAlias> = json(row, "computers_json")?;
        if text(row, "account_id")? != binding.scope.account_id
            || old.iter().any(|alias| !binding.computers.contains(alias))
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Synchronization identity cannot be rebound.",
            ));
        }
    }
    let computers = encode(&binding.computers)?;
    tx.execute("INSERT INTO sync_bindings(hub_id, account_id, source_user_id, computers_json) VALUES (?, ?, ?, ?) ON CONFLICT(hub_id, account_id, source_user_id) DO UPDATE SET computers_json = excluded.computers_json", &[&binding.scope.hub_id, &binding.scope.account_id, &binding.source_user_id, &computers]).await?;
    Ok(())
}

pub(crate) async fn append(
    tx: &mut dyn StorageTransaction,
    binding: &SyncBinding,
    resource: &SyncResourceKey,
    changes_json: &str,
    created_at: i64,
    capture_id: &str,
) -> TalosResult<()> {
    tx.execute("INSERT INTO sync_outbox(hub_id, account_id, source_user_id, kind, resource_id, changes_json, created_at, capture_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)", &[&binding.scope.hub_id, &binding.scope.account_id, &binding.source_user_id, resource.kind().as_str(), resource.id(), changes_json, &created_at.to_string(), capture_id]).await?;
    Ok(())
}

pub(crate) async fn has_capture_baseline(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
) -> TalosResult<bool> {
    let rows = tx.query(
        "SELECT 1 FROM visible_sync_outbox WHERE hub_id = ?1 AND account_id = ?2 AND kind = ?3 AND resource_id = ?4
         UNION ALL
         SELECT 1 FROM sync_documents WHERE hub_id = ?1 AND account_id = ?2 AND kind = ?3 AND resource_id = ?4
         LIMIT 1",
        &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()],
    ).await?;
    Ok(!rows.is_empty())
}

pub(crate) async fn exported_rows_through(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
    through: i64,
) -> TalosResult<Vec<(i64, SyncVersion)>> {
    let rows = tx.query("SELECT seq, exported_version_json FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? AND seq <= ? AND exported_version_json IS NOT NULL ORDER BY seq LIMIT 1024", &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id(), &through.to_string()]).await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.get_i64("seq").ok_or_else(corrupt_record)?,
                json(row, "exported_version_json")?,
            ))
        })
        .collect()
}

pub(crate) async fn outbox_through(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
    after: i64,
    through: i64,
    limit: usize,
) -> TalosResult<Vec<OutboxRecord>> {
    let rows = tx.query("SELECT seq, changes_json, exported_version_json FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? AND seq > ? AND seq <= ? ORDER BY seq LIMIT ?", &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id(), &after.to_string(), &through.to_string(), &limit.to_string()]).await?;
    rows.iter()
        .map(|row| {
            Ok(OutboxRecord {
                seq: row.get_i64("seq").ok_or_else(corrupt_record)?,
                changes: json(row, "changes_json")?,
                exported_version: row
                    .get_string("exported_version_json")
                    .map(|value| serde_json::from_str(&value).map_err(codec_error))
                    .transpose()?,
            })
        })
        .collect()
}

pub(crate) async fn outbox_tail(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
) -> TalosResult<Option<i64>> {
    let rows = tx.query("SELECT MAX(seq) AS tail FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()]).await?;
    Ok(rows.first().and_then(|row| row.get_i64("tail")))
}

pub(crate) fn encode<T: Serialize + ?Sized>(value: &T) -> TalosResult<String> {
    serde_json::to_string(value).map_err(codec_error)
}

pub(crate) fn json<T: DeserializeOwned>(row: &Row, field: &str) -> TalosResult<T> {
    serde_json::from_str(&text(row, field)?).map_err(codec_error)
}

pub(crate) fn text(row: &Row, field: &str) -> TalosResult<String> {
    row.get_string(field).ok_or_else(corrupt_record)
}

fn version(row: &Row) -> TalosResult<SyncVersion> {
    Ok(SyncVersion {
        version_vector: unhex(&text(row, "vector")?)?,
        history_generation: u64::try_from(row.get_i64("generation").ok_or_else(corrupt_record)?)
            .map_err(|_| corrupt_record())?,
    })
}

fn corrupt_record() -> TalosError {
    TalosError::internal("Read synchronization state; technical details were withheld.")
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789ABCDEF";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 15) as usize] as char);
    }
    result
}

fn unhex(value: &str) -> TalosResult<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(corrupt_record());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |value: u8| match value {
                b'0'..=b'9' => Ok(value - b'0'),
                b'A'..=b'F' => Ok(value - b'A' + 10),
                _ => Err(corrupt_record()),
            };
            Ok((digit(pair[0])? << 4) | digit(pair[1])?)
        })
        .collect()
}

/// Timestamp for local transport bookkeeping; native event timestamps are preserved by the Parser.
pub(crate) fn timestamp() -> TalosResult<i64> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| TalosError::internal_with_source("Read synchronization time.", error))?;
    i64::try_from(elapsed.as_secs()).map_err(|error| {
        TalosError::internal_with_source("Synchronization time exceeds the supported range.", error)
    })
}
