use std::sync::Arc;

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Storage, StorageTransaction};

use crate::{SyncResourceKey, SyncScope};

use super::{
    corrupt_record, hex,
    staged_write::{Kind, StagedWrite},
    text, unhex,
};

const CHUNK_BYTES: usize = 256 * 1024;

pub(crate) enum Snapshot {
    Inline(String),
    Staged(StagedWrite),
}

impl Snapshot {
    pub(crate) async fn prepare(storage: Arc<dyn Storage>, snapshot: Vec<u8>) -> TalosResult<Self> {
        if snapshot.len() <= CHUNK_BYTES {
            return Ok(Self::Inline(hex(&snapshot)));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut tx = storage.begin_background_transaction().await?;
        tx.execute(
            "INSERT INTO sync_snapshots(id,state,bytes) VALUES (?,'staged',?)",
            &[&id, &snapshot.len().to_string()],
        )
        .await?;
        tx.commit().await?;
        let staged = StagedWrite {
            id,
            storage,
            kind: Kind::Snapshot,
        };
        for (index, chunk) in snapshot.chunks(CHUNK_BYTES).enumerate() {
            let contents = hex(chunk);
            let mut tx = staged.storage.begin_background_transaction().await?;
            let changed = tx
                .execute(
                    "INSERT INTO sync_snapshot_chunks(snapshot_id,ordinal,contents)
                 SELECT id,?,unhex(?) FROM sync_snapshots WHERE id=? AND state='staged'",
                    &[&index.to_string(), &contents, &staged.id],
                )
                .await?;
            if changed != 1 {
                return Err(unavailable());
            }
            tx.commit().await?;
            tokio::task::yield_now().await;
        }
        Ok(Self::Staged(staged))
    }

    pub(super) fn inline_hex(&self) -> &str {
        match self {
            Self::Inline(bytes) => bytes,
            Self::Staged(_) => "",
        }
    }

    pub(super) fn id(&self) -> &str {
        match self {
            Self::Inline(_) => "",
            Self::Staged(snapshot) => &snapshot.id,
        }
    }

    pub(super) async fn publish(
        &self,
        tx: &mut dyn StorageTransaction,
        scope: &SyncScope,
        resource: &SyncResourceKey,
    ) -> TalosResult<()> {
        tx.execute(
            "UPDATE sync_snapshots SET state='retired' WHERE id=(
                SELECT snapshot_id FROM sync_documents WHERE hub_id=?1 AND account_id=?2 AND kind=?3 AND resource_id=?4
            ) AND NOT EXISTS (
                SELECT 1 FROM sync_documents d WHERE d.snapshot_id=sync_snapshots.id
                AND NOT (d.hub_id=?1 AND d.account_id=?2 AND d.kind=?3 AND d.resource_id=?4)
            )",
            &[&scope.hub_id, &scope.account_id, resource.kind().as_str(), resource.id()],
        ).await?;
        if let Self::Staged(snapshot) = self {
            snapshot.publish(tx).await?;
        }
        Ok(())
    }
}

pub(super) async fn read(tx: &mut dyn StorageTransaction, id: &str) -> TalosResult<Vec<u8>> {
    let rows = tx
        .query(
            "SELECT s.bytes, c.ordinal, hex(c.contents) AS chunk FROM sync_snapshots s
         JOIN sync_snapshot_chunks c ON c.snapshot_id=s.id WHERE s.id=? AND s.state='published'
         ORDER BY c.ordinal",
            &[id],
        )
        .await?;
    let bytes = rows
        .first()
        .and_then(|row| row.get_i64("bytes"))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(corrupt_record)?;
    let mut snapshot = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.get_i64("ordinal") != Some(index as i64) {
            return Err(corrupt_record());
        }
        snapshot.extend(unhex(&text(row, "chunk")?)?);
    }
    if snapshot.len() != bytes {
        return Err(corrupt_record());
    }
    Ok(snapshot)
}

pub(crate) async fn reclaim(storage: &dyn Storage) -> TalosResult<bool> {
    let rows = storage
        .query(
            "SELECT id FROM sync_snapshots WHERE state='retired' ORDER BY id LIMIT 1",
            &[],
        )
        .await?;
    let Some(row) = rows.first() else {
        return Ok(false);
    };
    let id = text(row, "id")?;
    let mut tx = storage.begin_background_transaction().await?;
    let removed = tx
        .execute(
            "DELETE FROM sync_snapshot_chunks WHERE snapshot_id=?1 AND ordinal=(
            SELECT MIN(ordinal) FROM sync_snapshot_chunks WHERE snapshot_id=?1
        )",
            &[&id],
        )
        .await?;
    if removed == 0 {
        tx.execute(
            "DELETE FROM sync_snapshots WHERE id=? AND state='retired'",
            &[&id],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The staged synchronization snapshot became unavailable.",
    )
}
