//! Shared checkpoint, version and durable acknowledgement persistence.
use super::*;

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
    let rows = tx.query("SELECT replica_schema_version, snapshot_id, hex(snapshot_blob) AS snapshot, hex(version_vector) AS vector, generation, hex(acknowledged_vector) AS acknowledged, acknowledged_generation, session_projection_json IS NOT NULL AS has_session_projection FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()]).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let snapshot = match row.get_string("snapshot_id") {
        Some(id) => snapshot::read(tx, &id).await?,
        None => unhex(&text(row, "snapshot")?)?,
    };
    Ok(Some(DocumentRecord {
        snapshot,
        schema_version: row
            .get_i64("replica_schema_version")
            .ok_or_else(corrupt_record)?,
        version: version(row)?,
        acknowledged: acknowledged_version(row)?,
        has_session_projection: row.get_i64("has_session_projection") == Some(1),
    }))
}

pub(crate) async fn save_document(
    tx: &mut dyn StorageTransaction,
    scope: &SyncScope,
    summary: &SyncResourceSummary,
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
            acknowledged_generation, session_projection_json, replica_schema_version
        ) VALUES (?, ?, ?, ?, NULLIF(?, ''), ?, ?, unhex(?), NULLIF(?, ''), unhex(?), unhex(?), ?, NULLIF(?, ''), 2)
        ON CONFLICT(hub_id, account_id, kind, resource_id) DO UPDATE SET
            replica_schema_version = 2, computer_id = excluded.computer_id, generation = excluded.generation,
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

/// Retain canonical bytes and versions across Hubs; the new Hub acknowledges its own baseline.
pub(crate) async fn copy_checkpoint_scope(
    tx: &mut dyn StorageTransaction,
    previous: &SyncScope,
    next: &SyncScope,
) -> TalosResult<()> {
    tx.execute(
        "INSERT INTO sync_documents(hub_id,account_id,kind,resource_id,computer_id,generation,deleted,
            snapshot_blob,snapshot_id,version_vector,acknowledged_vector,acknowledged_generation,session_projection_json,replica_schema_version)
         SELECT ?1,account_id,kind,resource_id,computer_id,generation,deleted,
            snapshot_blob,snapshot_id,version_vector,unhex(''),0,session_projection_json,replica_schema_version
         FROM sync_documents WHERE hub_id=?2 AND account_id=?3",
        &[&next.hub_id, &previous.hub_id, &next.account_id],
    ).await?;
    Ok(())
}
