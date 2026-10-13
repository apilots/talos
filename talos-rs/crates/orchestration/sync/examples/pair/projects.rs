use std::sync::Arc;

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{SqliteStorage, Storage, StorageTransaction};
use talos_sync::{
    SyncCaptureParams, SyncChange, SyncDataAdapter, SyncExportParams, SyncImportBatch,
    SyncResourceKey, SyncResourceSnapshot, SyncScanPage, SyncScanParams,
};
use talos_types::ProjectInfo;

use super::Peer;

pub(super) async fn capture_project(peer: &Peer, project_id: &str) -> TalosResult<()> {
    let resource = SyncResourceKey::Project {
        project_id: project_id.into(),
    };
    let mut transaction = peer.storage.begin_transaction().await?;
    let changes = peer
        .adapter
        .export(
            &mut *transaction,
            SyncExportParams {
                binding: peer.binding.clone(),
                resource: resource.clone(),
            },
        )
        .await?;
    peer.sync
        .capture(
            &mut *transaction,
            SyncCaptureParams {
                binding: peer.binding.clone(),
                resource,
                changes,
                created_at: 100,
            },
        )
        .await?;
    transaction.commit().await
}

pub(super) async fn delete_project(peer: &Peer, project_id: &str) -> TalosResult<()> {
    let mut transaction = peer.storage.begin_transaction().await?;
    transaction
        .execute(
            "DELETE FROM projects WHERE project_id = ? AND user_id = ?",
            &[project_id, &peer.binding.scope.account_id],
        )
        .await?;
    peer.sync
        .capture(
            &mut *transaction,
            SyncCaptureParams {
                binding: peer.binding.clone(),
                resource: SyncResourceKey::Project {
                    project_id: project_id.into(),
                },
                changes: vec![SyncChange::Delete],
                created_at: 100,
            },
        )
        .await?;
    transaction.commit().await
}

pub(super) struct ProjectAdapter {
    pub(super) storage: Arc<SqliteStorage>,
}

impl SyncDataAdapter for ProjectAdapter {
    async fn scan(&self, params: SyncScanParams) -> TalosResult<SyncScanPage> {
        let rows = self.storage.query("SELECT project_id FROM projects WHERE user_id = ? AND project_id > ? ORDER BY project_id LIMIT ?", &[&params.binding.source_user_id, params.cursor.as_deref().unwrap_or_default(), &params.limit.to_string()]).await?;
        let resources = rows
            .iter()
            .map(|row| SyncResourceKey::Project {
                project_id: row.get_string("project_id").expect("persisted project ID"),
            })
            .collect::<Vec<_>>();
        let next_cursor =
            (resources.len() == params.limit).then(|| resources.last().unwrap().id().into());
        Ok(SyncScanPage {
            resources,
            next_cursor,
        })
    }

    async fn export(
        &self,
        tx: &mut dyn StorageTransaction,
        params: SyncExportParams,
    ) -> TalosResult<Vec<SyncChange>> {
        let row = tx.query("SELECT project_id, execution_node_id, name, directory, created_at, updated_at FROM projects WHERE project_id = ? AND user_id = ?", &[params.resource.id(), &params.binding.source_user_id]).await?;
        let row = row.first().ok_or_else(|| {
            TalosError::new(ErrorCode::ResourceNotFound, "Project is unavailable.")
        })?;
        let project = ProjectInfo {
            node_id: row.get_string("execution_node_id").unwrap(),
            project_id: row.get_string("project_id").unwrap(),
            name: row.get_string("name").unwrap(),
            directory: row.get_string("directory").unwrap().into(),
            status: Default::default(),
            created_at: row.get_i64("created_at").unwrap(),
            updated_at: row.get_i64("updated_at").unwrap(),
            session_count: 0,
        };
        Ok(vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Project(project)),
        }])
    }

    async fn apply(
        &self,
        tx: &mut dyn StorageTransaction,
        batch: &SyncImportBatch,
    ) -> TalosResult<()> {
        for change in &batch.changes {
            match change {
                talos_sync_types::SyncChange::Metadata { after, .. } => {
                    let talos_sync_types::SyncResourceSnapshot::Project(project) = after.as_ref()
                    else {
                        continue;
                    };
                    tx.execute("INSERT INTO projects(project_id, execution_node_id, user_id, name, directory, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(project_id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at", &[&project.project_id, &project.node_id, &batch.scope.account_id, &project.name, &project.directory, &project.created_at.to_string(), &project.updated_at.to_string()]).await?;
                }
                talos_sync_types::SyncChange::Delete => {
                    tx.execute(
                        "DELETE FROM projects WHERE project_id = ? AND user_id = ?",
                        &[batch.resource.id(), &batch.scope.account_id],
                    )
                    .await?;
                }
                talos_sync_types::SyncChange::Turn(_)
                | talos_sync_types::SyncChange::Event(_)
                | talos_sync_types::SyncChange::ActiveTurnMetadata(_)
                | talos_sync_types::SyncChange::AcceptedCommand(_)
                | talos_sync_types::SyncChange::Reset { .. } => {}
            }
        }
        Ok(())
    }
}

pub(super) async fn rename(peer: &Peer, project_id: &str, name: &str) -> TalosResult<()> {
    let params = SyncExportParams {
        binding: peer.binding.clone(),
        resource: SyncResourceKey::Project {
            project_id: project_id.into(),
        },
    };
    let mut tx = peer.storage.begin_transaction().await?;
    let before = peer
        .adapter
        .export(
            &mut *tx,
            SyncExportParams {
                binding: params.binding.clone(),
                resource: params.resource.clone(),
            },
        )
        .await?;
    tx.execute(
        "UPDATE projects SET name = ? WHERE project_id = ? AND user_id = ?",
        &[name, project_id, &peer.binding.scope.account_id],
    )
    .await?;
    let after = peer.adapter.export(&mut *tx, params).await?;
    let (SyncChange::Metadata { after: before, .. }, SyncChange::Metadata { after, .. }) = (
        before.into_iter().next().unwrap(),
        after.into_iter().next().unwrap(),
    ) else {
        unreachable!()
    };
    peer.sync
        .capture(
            &mut *tx,
            SyncCaptureParams {
                binding: peer.binding.clone(),
                resource: SyncResourceKey::Project {
                    project_id: project_id.into(),
                },
                changes: vec![SyncChange::Metadata {
                    before: Some(before),
                    after,
                }],
                created_at: 100,
            },
        )
        .await?;
    tx.commit().await
}
