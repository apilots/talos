//! Authorized facts at the original Store transaction boundary.
//! Imports use these Store-owned projections without invoking Services, Agents,
//! accounting, filesystem mutation, continuation claims or capture again.

mod bindings;
pub(crate) mod capture;
mod catalog;
mod collect;
pub use bindings::StoreSyncBindings;
mod file;
mod import_stage;
mod metadata;
mod reclaim;
pub(crate) mod session;
pub(crate) mod source;
mod timeline;

use std::sync::{Arc, RwLock, Weak};
use talos_error::{TalosError, TalosResult};
use talos_storage::{Storage, StorageTransaction};
use talos_sync::{
    SyncBinding, SyncChange, SyncDataAdapter, SyncExportFacts, SyncExportParams, SyncHandle,
    SyncImportBatch, SyncResourceKey, SyncResourceSnapshot, SyncScanPage, SyncScanParams,
    TalosSync,
};

/// Original Service projection for the bound Worker's model and permissions.
/// The callback must use that Worker configuration, including on another host.
pub type SessionInfoProjection =
    dyn Fn(&crate::session::SessionRecord) -> TalosResult<talos_types::SessionInfo> + Send + Sync;

/// The one product bridge for an installed, authenticated account binding.
/// The caller owns account verification and network lifecycle. It must install
/// this bridge on the original Stores before enabling product mutations.
pub struct StoreSyncAdapter {
    storage: Arc<dyn Storage>,
    pub(crate) sync: Arc<TalosSync>,
    pub(crate) binding: SyncBinding,
    handle: RwLock<Weak<SyncHandle>>,
    capture_wakeup: Arc<tokio::sync::Notify>,
    session_projection: Arc<SessionInfoProjection>,
}

impl StoreSyncAdapter {
    pub fn new(
        storage: Arc<dyn Storage>,
        sync: Arc<TalosSync>,
        binding: SyncBinding,
        session_projection: Arc<SessionInfoProjection>,
    ) -> Self {
        Self {
            storage,
            sync,
            binding,
            session_projection,
            handle: RwLock::new(Weak::new()),
            capture_wakeup: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Wake the existing runner after a business commit; never await its network.
    pub fn attach_handle(&self, handle: Weak<SyncHandle>) {
        *self.handle.write().expect("sync handle lock") = handle;
    }

    pub(crate) fn committed(&self) {
        if let Some(handle) = self.handle.read().expect("sync handle lock").upgrade() {
            handle.notify_committed();
        }
    }

    pub(crate) fn wake_capture(&self) {
        self.capture_wakeup.notify_one();
    }

    pub fn binding(&self) -> &SyncBinding {
        &self.binding
    }

    pub(crate) fn local_node(&self, computer: &str) -> String {
        self.binding
            .computers
            .iter()
            .find(|alias| alias.computer_id == computer)
            .map_or_else(|| computer.to_owned(), |alias| alias.source_node_id.clone())
    }

    pub(crate) fn computer(&self, node: &str) -> String {
        self.binding
            .computers
            .iter()
            .find(|alias| alias.source_node_id == node)
            .map_or_else(|| node.to_owned(), |alias| alias.computer_id.clone())
    }

    pub(crate) async fn generation(
        &self,
        tx: &mut dyn StorageTransaction,
        resource: &SyncResourceKey,
    ) -> TalosResult<u64> {
        if matches!(resource, SyncResourceKey::Session { .. }) {
            if let Some(generation) = tx
                .query(
                    "SELECT history_generation FROM sessions WHERE session_key = ?",
                    &[resource.id()],
                )
                .await?
                .first()
                .and_then(|row| row.get_i64("history_generation"))
            {
                return Ok(generation as u64);
            }
        }
        let rows = tx.query("SELECT MAX(generation) AS generation FROM (SELECT generation FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? UNION ALL SELECT CAST(json_extract(value, '$.data.historyGeneration') AS INTEGER) FROM visible_sync_outbox, json_each(changes_json) WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? AND json_extract(value, '$.type') = 'reset')", &[&self.binding.scope.hub_id, &self.binding.scope.account_id, resource.kind().as_str(), resource.id(), &self.binding.scope.hub_id, &self.binding.scope.account_id, resource.kind().as_str(), resource.id()]).await?;
        Ok(rows
            .first()
            .and_then(|r| r.get_i64("generation"))
            .unwrap_or(0) as u64)
    }

    async fn ensure_import_ready(
        &self,
        tx: &mut dyn StorageTransaction,
        batch: &SyncImportBatch,
    ) -> TalosResult<()> {
        if batch.scope != self.binding.scope {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Synchronization scope does not match this Store.",
            ));
        }
        if self.has_source_gap(tx, &batch.resource).await? {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Local resource facts must be captured before importing synchronization.",
            ));
        }
        if let SyncResourceKey::Session { session_id } = &batch.resource {
            let executing = tx
                .query(
                    "SELECT 1 FROM agent_active_turns WHERE session_id=?",
                    &[session_id],
                )
                .await?;
            if !executing.is_empty() {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "The local Turn must settle before importing synchronization.",
                ));
            }
        }
        Ok(())
    }
}

impl SyncDataAdapter for StoreSyncAdapter {
    async fn scan(&self, params: SyncScanParams) -> TalosResult<SyncScanPage> {
        if params.binding != self.binding {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Synchronization binding does not match this Store.",
            ));
        }
        catalog::scan(&*self.storage, params).await
    }

    async fn export(
        &self,
        tx: &mut dyn StorageTransaction,
        params: SyncExportParams,
    ) -> TalosResult<SyncExportFacts> {
        if params.binding != self.binding {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Synchronization binding does not match this Store.",
            ));
        }
        if self.has_source_gap(tx, &params.resource).await? {
            return Ok(SyncExportFacts::default());
        }
        if let SyncResourceKey::Computer { computer_id } = &params.resource {
            if !tx
                .query(
                    "SELECT 1 FROM computer_deletions WHERE node_id = ? AND owner_user_id = ?",
                    &[computer_id, &params.binding.scope.account_id],
                )
                .await?
                .is_empty()
            {
                return Ok(SyncExportFacts {
                    changes: vec![SyncChange::Delete],
                    ..Default::default()
                });
            }
        }
        if let SyncResourceKey::Session { session_id } = &params.resource {
            let Some(row) = metadata::session_row(self, tx, session_id).await? else {
                return Ok(SyncExportFacts::default());
            };
            let cut = source::cut_from_row(tx, session_id, row).await?;
            return self
                .project_revision(
                    tx,
                    session_id,
                    false,
                    source::SessionRevision {
                        before: None,
                        after: Some(cut),
                    },
                )
                .await;
        }
        let Some(source) = metadata::read_source(self, tx, &params.resource).await? else {
            return Ok(SyncExportFacts::default());
        };
        Ok(SyncExportFacts {
            changes: vec![SyncChange::Metadata {
                before: None,
                after: Box::new(source.project(self, &params.resource)?),
            }],
            ..Default::default()
        })
    }

    async fn missing_dependencies(
        &self,
        tx: &mut dyn StorageTransaction,
        batch: &SyncImportBatch,
    ) -> TalosResult<Vec<SyncResourceKey>> {
        let mut dependencies = Vec::new();
        for change in &batch.changes {
            let SyncChange::Metadata { after, .. } = change else {
                continue;
            };
            let mut required = Vec::new();
            match after.as_ref() {
                SyncResourceSnapshot::Computer(_) => {}
                SyncResourceSnapshot::Project(_) => {}
                SyncResourceSnapshot::Session(s) => {
                    if let Some(id) = &s.session.project_id {
                        required.push(SyncResourceKey::Project {
                            project_id: id.clone(),
                        });
                    }
                    if let talos_types::SessionTreeIdentity::Subagent {
                        parent,
                        tree_root_session_id,
                        ..
                    } = &s.tree_identity
                    {
                        required.push(SyncResourceKey::Session {
                            session_id: parent.session_id.clone(),
                        });
                        required.push(SyncResourceKey::Session {
                            session_id: tree_root_session_id.clone(),
                        });
                    }
                }
                SyncResourceSnapshot::File(f) => {
                    if let talos_types::FileScope::Project { project_id: id } = &f.file.scope {
                        required.push(SyncResourceKey::Project {
                            project_id: id.clone(),
                        });
                    }
                }
            }
            for key in required {
                let (table, column, id) = match &key {
                    SyncResourceKey::Computer { computer_id } => {
                        ("agent_nodes", "node_id", self.local_node(computer_id))
                    }
                    SyncResourceKey::Project { project_id } => {
                        ("projects", "project_id", project_id.clone())
                    }
                    SyncResourceKey::Session { session_id } => {
                        ("sessions", "session_key", session_id.clone())
                    }
                    SyncResourceKey::File { file_id } => ("files", "file_id", file_id.clone()),
                };
                if tx
                    .query(&format!("SELECT 1 FROM {table} WHERE {column} = ?"), &[&id])
                    .await?
                    .is_empty()
                    && !dependencies.contains(&key)
                {
                    dependencies.push(key);
                }
            }
        }
        Ok(dependencies)
    }

    async fn prepare_import(&self, batch: &SyncImportBatch) -> TalosResult<()> {
        let mut read = self.storage.begin_read_transaction().await?;
        self.ensure_import_ready(&mut *read, batch).await?;
        read.commit().await?;
        import_stage::prepare(self, batch).await
    }

    async fn apply(
        &self,
        tx: &mut dyn StorageTransaction,
        batch: &SyncImportBatch,
    ) -> TalosResult<()> {
        self.ensure_import_ready(tx, batch).await?;
        if batch
            .changes
            .iter()
            .any(|c| matches!(c, SyncChange::Delete))
        {
            metadata::delete(self, tx, &batch.resource).await?;
            return Ok(());
        }
        import_stage::publish(tx, batch).await?;
        metadata::apply(self, tx, batch).await?;
        if matches!(batch.resource, SyncResourceKey::Session { .. }) {
            session::apply(tx, batch).await?;
        }
        Ok(())
    }
}

pub(crate) fn codec(error: serde_json::Error) -> TalosError {
    TalosError::internal_with_source(
        "Process synchronized facts; technical details were withheld.",
        error,
    )
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
