use super::{source::SourceFields, ResourceKey};
use talos_error::TalosResult;
use talos_storage::StorageTransaction;
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MetadataSource {
    pub(crate) fields: SourceFields,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tags: Vec<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MetadataRevision {
    pub(crate) before: Option<MetadataSource>,
    pub(crate) after: Option<MetadataSource>,
}
pub(crate) async fn read(
    tx: &mut dyn StorageTransaction,
    resource: &ResourceKey,
) -> TalosResult<Option<MetadataSource>> {
    let rows = match resource {
        ResourceKey::Project { project_id } => tx.query("SELECT p.user_id AS owner_user_id, p.execution_node_id, p.name, p.directory, p.created_at, p.updated_at, (SELECT COUNT(*) FROM sessions s WHERE s.project_id=p.project_id) AS session_count FROM projects p WHERE p.project_id=?", &[project_id]).await?,
        ResourceKey::File { file_id } => tx.query("SELECT owner_user_id, project_id, storage_peer_id, original_filename, mime_type, size_bytes, metadata_json, created_at, updated_at FROM files WHERE file_id=? AND status='ready'", &[file_id]).await?,
    };
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let tags = if matches!(resource, ResourceKey::File { .. }) {
        tx.query(
            "SELECT tag FROM file_tags WHERE file_id=? ORDER BY tag",
            &[resource.id()],
        )
        .await?
        .iter()
        .map(|row| super::text(row, "tag"))
        .collect::<TalosResult<Vec<_>>>()?
    } else {
        Vec::new()
    };
    Ok(Some(MetadataSource {
        fields: row.into(),
        tags,
    }))
}
