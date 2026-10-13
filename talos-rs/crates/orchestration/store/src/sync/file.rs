use super::{
    codec,
    metadata::{number, text},
    StoreSyncAdapter,
};
use std::collections::BTreeMap;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use talos_sync::{SyncFileSnapshot, SyncImportBatch, SyncResourceSnapshot};
use talos_types::{FileInfo, FileScope};

pub(super) async fn read(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    file_id: &str,
) -> TalosResult<Option<SyncResourceSnapshot>> {
    let rows = tx.query("SELECT * FROM files WHERE file_id = ? AND owner_user_id IN (?, ?) AND status = 'ready'", &[file_id, &adapter.binding.source_user_id, &adapter.binding.scope.account_id]).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let tags = tx
        .query(
            "SELECT tag FROM file_tags WHERE file_id = ? ORDER BY tag",
            &[file_id],
        )
        .await?
        .iter()
        .map(|r| text(r, "tag"))
        .collect::<TalosResult<Vec<_>>>()?;
    let metadata: BTreeMap<String, String> =
        serde_json::from_str(&text(row, "metadata_json")?).map_err(codec)?;
    let scope = row
        .get_string("project_id")
        .map_or(FileScope::Personal, |project_id| FileScope::Project {
            project_id,
        });
    Ok(Some(SyncResourceSnapshot::File(SyncFileSnapshot {
        file: FileInfo {
            file_id: file_id.into(),
            scope,
            original_filename: text(row, "original_filename")?,
            mime_type: text(row, "mime_type")?,
            size_bytes: number(row, "size_bytes")?
                .try_into()
                .map_err(|_| TalosError::internal("Invalid persisted file size."))?,
            tags,
            metadata,
            created_at: number(row, "created_at")?,
            updated_at: number(row, "updated_at")?,
        },
        storage_peer_id: row
            .get_string("storage_peer_id")
            .unwrap_or_else(|| adapter.binding.peer_id.clone()),
    })))
}

pub(super) async fn apply(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
    snapshot: &talos_sync_types::SyncFileSnapshot,
) -> TalosResult<()> {
    let file = &snapshot.file;
    let rows = tx
        .query(
            "SELECT owner_user_id, storage_peer_id FROM files WHERE file_id = ?",
            &[&file.file_id],
        )
        .await?;
    if rows.first().is_some_and(|r| {
        ![&batch.scope.account_id, &adapter.binding.source_user_id]
            .contains(&&r.get_string("owner_user_id").unwrap_or_default())
            || r.get_string("storage_peer_id")
                .unwrap_or_else(|| adapter.binding.peer_id.clone())
                != snapshot.storage_peer_id
    }) {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "File identity has a different body source.",
        ));
    }
    let project = match &file.scope {
        talos_sync_types::generated::FileScope::Personal => None,
        talos_sync_types::generated::FileScope::Project(project) => Some(project.as_str()),
    };
    let metadata = serde_json::to_string(&file.metadata).map_err(codec)?;
    if snapshot.storage_peer_id == adapter.binding.peer_id && rows.is_empty() {
        return Err(TalosError::new(
            ErrorCode::ServiceUnavailable,
            "Source FileService no longer has this file.",
        ));
    }
    tx.execute("INSERT INTO files(file_id, owner_user_id, project_id, storage_peer_id, original_filename, mime_type, size_bytes, status, metadata_json, created_at, updated_at) VALUES (?, ?, NULLIF(?, ''), ?, ?, ?, ?, 'ready', ?, ?, ?) ON CONFLICT(file_id) DO UPDATE SET metadata_json = excluded.metadata_json, updated_at = excluded.updated_at", &[&file.file_id, &batch.scope.account_id, project.unwrap_or_default(), &snapshot.storage_peer_id, &file.original_filename, &file.mime_type, &file.size_bytes.to_string(), &metadata, &file.created_at.to_string(), &file.updated_at.to_string()]).await?;
    tx.execute("DELETE FROM file_tags WHERE file_id = ?", &[&file.file_id])
        .await?;
    for tag in &file.tags {
        tx.execute(
            "INSERT INTO file_tags(file_id, tag) VALUES (?, ?)",
            &[&file.file_id, tag],
        )
        .await?;
    }
    Ok(())
}
