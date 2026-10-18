//! Authorized facts at the original Store transaction boundary.
//! Imports use these Store-owned projections without invoking Services, Agents,
//! accounting, filesystem mutation, continuation claims or capture again.

mod bindings;
pub(crate) mod capture;
mod catalog;
pub use bindings::StoreSyncBindings;
mod file;
mod metadata;
pub(crate) mod session;
mod timeline;

use std::sync::{Arc, RwLock, Weak};
use talos_error::{TalosError, TalosResult};
use talos_storage::{Storage, StorageTransaction};
use talos_sync::{
    SyncBinding, SyncChange, SyncDataAdapter, SyncExportParams, SyncHandle, SyncImportBatch,
    SyncResourceKey, SyncScanPage, SyncScanParams, TalosSync,
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
        let rows = tx.query("SELECT MAX(generation) AS generation FROM (SELECT generation FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? UNION ALL SELECT CAST(json_extract(value, '$.data.historyGeneration') AS INTEGER) FROM sync_outbox, json_each(changes_json) WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? AND json_extract(value, '$.type') = 'reset')", &[&self.binding.scope.hub_id, &self.binding.scope.account_id, resource.kind().as_str(), resource.id(), &self.binding.scope.hub_id, &self.binding.scope.account_id, resource.kind().as_str(), resource.id()]).await?;
        Ok(rows
            .first()
            .and_then(|r| r.get_i64("generation"))
            .unwrap_or(0) as u64)
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
    ) -> TalosResult<Vec<SyncChange>> {
        if params.binding != self.binding {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Synchronization binding does not match this Store.",
            ));
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
                return Ok(vec![SyncChange::Delete]);
            }
        }
        let mut changes = match metadata::read(self, tx, &params.resource).await? {
            Some(after) => vec![SyncChange::Metadata {
                before: None,
                after: Box::new(after),
            }],
            None => return Ok(Vec::new()),
        };
        if let SyncResourceKey::Session { session_id } = &params.resource {
            changes.extend(timeline::export(self, tx, session_id, 0).await?);
        }
        Ok(changes)
    }

    async fn missing_dependencies(
        &self,
        tx: &mut dyn StorageTransaction,
        batch: &SyncImportBatch,
    ) -> TalosResult<Vec<SyncResourceKey>> {
        let mut dependencies = Vec::new();
        for change in &batch.changes {
            let talos_sync_types::SyncChange::Metadata { after, .. } = change else {
                continue;
            };
            let mut required = Vec::new();
            match after.as_ref() {
                talos_sync_types::SyncResourceSnapshot::Computer(_) => {}
                talos_sync_types::SyncResourceSnapshot::Project(p) => {
                    required.push(SyncResourceKey::Computer {
                        computer_id: p.node_id.clone(),
                    })
                }
                talos_sync_types::SyncResourceSnapshot::Session(s) => {
                    required.push(SyncResourceKey::Computer {
                        computer_id: s.binding.computer_id.clone(),
                    });
                    if let Some(id) = &s.session.project_id {
                        required.push(SyncResourceKey::Project {
                            project_id: id.clone(),
                        });
                    }
                    if let talos_sync_types::generated::SessionTreeIdentity::Subagent {
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
                talos_sync_types::SyncResourceSnapshot::File(f) => {
                    if let talos_sync_types::generated::FileScope::Project(id) = &f.file.scope {
                        required.push(SyncResourceKey::Project {
                            project_id: id.clone(),
                        });
                    }
                }
            }
            for key in required {
                if let SyncResourceKey::Computer { computer_id } = &key {
                    if !tx.query("SELECT 1 FROM computer_deletions WHERE node_id = ? AND owner_user_id = ?", &[computer_id, &batch.scope.account_id]).await?.is_empty() {
                        continue;
                    }
                }
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

    async fn apply(
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
        if batch
            .changes
            .iter()
            .any(|c| matches!(c, talos_sync_types::SyncChange::Delete))
        {
            metadata::delete(self, tx, &batch.resource).await?;
            return Ok(());
        }
        metadata::apply(self, tx, batch).await?;
        if matches!(batch.resource, SyncResourceKey::Session { .. }) {
            session::apply(self, tx, batch).await?;
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

pub(crate) fn transcode<T: serde::Serialize, U: serde::de::DeserializeOwned>(
    value: &T,
) -> TalosResult<U> {
    serde_json::from_value(serde_json::to_value(value).map_err(codec)?).map_err(codec)
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
