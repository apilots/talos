use serde::{de::DeserializeOwned, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
use talos_sync_types::{
    ComputerAlias, SyncBinding, SyncResourceKey, SyncResourceSummary, SyncScope, SyncVersion,
};

use crate::projection::codec_error;

pub(crate) mod snapshot;
pub(crate) mod staged_write;

pub(crate) struct DocumentRecord {
    pub snapshot: Vec<u8>,
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

pub(crate) async fn document_checkpoint(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
) -> TalosResult<Option<(SyncVersion, SyncVersion)>> {
    let rows = tx.query("SELECT hex(version_vector) AS vector, generation, hex(acknowledged_vector) AS acknowledged, acknowledged_generation FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()]).await?;
    rows.first()
        .map(|row| Ok((version(row)?, acknowledged_version(row)?)))
        .transpose()
}

pub(crate) async fn document_version(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
) -> TalosResult<Option<SyncVersion>> {
    let rows = tx.query("SELECT hex(version_vector) AS vector, generation FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()]).await?;
    rows.first().map(version).transpose()
}

pub(crate) async fn resource_summary(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
) -> TalosResult<Option<SyncResourceSummary>> {
    let rows = tx.query(
        "SELECT computer_id, deleted, hex(version_vector) AS vector, generation FROM sync_documents WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=?",
        &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()],
    ).await?;
    rows.first()
        .map(|row| {
            Ok(SyncResourceSummary {
                resource: resource.clone(),
                computer_id: row.get_string("computer_id").filter(|id| !id.is_empty()),
                version: version(row)?,
                deleted: row.get_i64("deleted") == Some(1),
            })
        })
        .transpose()
}

pub(crate) async fn document(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
) -> TalosResult<Option<DocumentRecord>> {
    let rows = tx.query("SELECT snapshot_id, hex(snapshot_blob) AS snapshot, hex(version_vector) AS vector, generation, hex(acknowledged_vector) AS acknowledged, acknowledged_generation, session_projection_json IS NOT NULL AS has_session_projection FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()]).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let snapshot = match row.get_string("snapshot_id") {
        Some(id) => snapshot::read(tx, &id).await?,
        None => unhex(&text(row, "snapshot")?)?,
    };
    Ok(Some(DocumentRecord {
        snapshot,
        version: version(row)?,
        acknowledged: acknowledged_version(row)?,
        has_session_projection: row.get_i64("has_session_projection") == Some(1),
    }))
}

pub(crate) async fn save_document(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    summary: &talos_sync_types::SyncResourceSummary,
    snapshot: &snapshot::Snapshot,
    acknowledged: &SyncVersion,
    session_projection: Option<&str>,
) -> TalosResult<()> {
    let generation = i64::try_from(summary.version.history_generation).map_err(|_| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "History generation exceeds the persistence limit.",
        )
    })?;
    snapshot.publish(tx, scope, &summary.resource).await?;
    // Talos's bundled SQLite 3.44 accepts bound hexadecimal bytes through
    // unhex(); the legacy transaction interface otherwise binds text only.
    let sql = "
        INSERT INTO sync_documents(
            hub_id, account_id, kind, resource_id, computer_id, generation, deleted,
            snapshot_blob, snapshot_id, version_vector, acknowledged_vector,
            acknowledged_generation, session_projection_json
        ) VALUES (?, ?, ?, ?, NULLIF(?, ''), ?, ?, unhex(?), NULLIF(?, ''), unhex(?), unhex(?), ?, NULLIF(?, ''))
        ON CONFLICT(hub_id, account_id, kind, resource_id) DO UPDATE SET
            computer_id = excluded.computer_id, generation = excluded.generation,
            deleted = excluded.deleted, snapshot_blob = excluded.snapshot_blob,
            snapshot_id = excluded.snapshot_id, version_vector = excluded.version_vector,
            acknowledged_vector = excluded.acknowledged_vector,
            acknowledged_generation = excluded.acknowledged_generation,
            session_projection_json = COALESCE(excluded.session_projection_json, sync_documents.session_projection_json)";
    let acknowledged_generation =
        i64::try_from(acknowledged.history_generation).map_err(|_| corrupt_record())?;
    tx.execute(
        sql,
        &[
            &scope.hub_id,
            &scope.account_id,
            summary.resource.kind().as_str(),
            summary.resource.id(),
            summary.computer_id.as_deref().unwrap_or_default(),
            &generation.to_string(),
            if summary.deleted { "1" } else { "0" },
            snapshot.inline_hex(),
            snapshot.id(),
            &hex(&summary.version.version_vector),
            &hex(&acknowledged.version_vector),
            &acknowledged_generation.to_string(),
            session_projection.unwrap_or_default(),
        ],
    )
    .await?;
    Ok(())
}

pub(crate) async fn acknowledgement(
    tx: &mut dyn StorageTransaction,
    batch: &crate::SyncExportBatch,
) -> TalosResult<Option<AcknowledgementRecord>> {
    // A missing outbox row is an already committed ACK. A missing document for
    // an existing row is corrupt state, so keep the LEFT JOIN and validate it.
    let rows = tx.query("SELECT o.exported_version_json, hex(d.version_vector) AS vector, d.generation, hex(d.acknowledged_vector) AS acknowledged, d.acknowledged_generation FROM visible_sync_outbox o LEFT JOIN sync_documents d ON d.hub_id = o.hub_id AND d.account_id = o.account_id AND d.kind = o.kind AND d.resource_id = o.resource_id WHERE o.seq = ? AND o.hub_id = ? AND o.account_id = ? AND o.kind = ? AND o.resource_id = ?", &[&batch.through_sequence.to_string(), &batch.scope.hub_id, &batch.scope.account_id, batch.resource.kind().as_str(), batch.resource.id()]).await?;
    rows.first()
        .map(|row| {
            Ok(AcknowledgementRecord {
                exported: json(row, "exported_version_json")?,
                current: version(row)?,
                acknowledged: acknowledged_version(row)?,
            })
        })
        .transpose()
}

pub(crate) async fn save_acknowledgement(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    resource: &SyncResourceKey,
    acknowledged: &SyncVersion,
) -> TalosResult<()> {
    let generation =
        i64::try_from(acknowledged.history_generation).map_err(|_| corrupt_record())?;
    let affected = tx.execute("UPDATE sync_documents SET acknowledged_vector = unhex(?), acknowledged_generation = ? WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&hex(&acknowledged.version_vector), &generation.to_string(), &scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()]).await?;
    if affected != 1 {
        return Err(corrupt_record());
    }
    Ok(())
}

fn acknowledged_version(row: &Row) -> TalosResult<SyncVersion> {
    Ok(SyncVersion {
        version_vector: unhex(&text(row, "acknowledged")?)?,
        history_generation: u64::try_from(
            row.get_i64("acknowledged_generation")
                .ok_or_else(corrupt_record)?,
        )
        .map_err(|_| corrupt_record())?,
    })
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
        .chunks_exact(2)
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
