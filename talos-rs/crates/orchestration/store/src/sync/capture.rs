use super::{codec, metadata, StoreSyncAdapter, StoreSyncBindings};
use async_trait::async_trait;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_storage::{Row, StorageTransaction};
use talos_sync::{SyncChange, SyncExportFacts, SyncResourceKey};

pub(crate) struct ResourceCut {
    resource: SyncResourceKey,
    metadata: Option<metadata::MetadataSource>,
}

/// Decorate exactly the Store's existing transaction, not SQL or a second DB.
/// Scope and resource keys come from the Store command, never from SQL parsing.
pub(crate) async fn transaction(
    mut tx: Box<dyn StorageTransaction>,
    bindings: Arc<StoreSyncBindings>,
    resources: Vec<SyncResourceKey>,
) -> TalosResult<Box<dyn StorageTransaction>> {
    let mut cuts = Vec::new();
    for adapter in bindings.all() {
        let prepared = prepare(&adapter, &mut *tx, resources.clone()).await?;
        cuts.push((adapter, prepared));
    }
    if cuts.is_empty() {
        return Ok(tx);
    }
    Ok(Box::new(CaptureTransaction { tx, cuts }))
}

pub(crate) async fn prepare(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    resources: Vec<SyncResourceKey>,
) -> TalosResult<Vec<ResourceCut>> {
    let mut cuts = Vec::new();
    for resource in resources {
        let metadata = metadata::read_source(adapter, tx, &resource).await?;
        cuts.push(ResourceCut { resource, metadata });
    }
    Ok(cuts)
}

pub(super) async fn has_baseline(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    resource: &SyncResourceKey,
) -> TalosResult<bool> {
    let canonical_id = match resource {
        SyncResourceKey::Computer { computer_id } => adapter.computer(computer_id),
        _ => resource.id().to_owned(),
    };
    let rows = tx.query("SELECT 1 AS present FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? UNION ALL SELECT 1 FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? LIMIT 1", &[&adapter.binding.scope.hub_id, &adapter.binding.scope.account_id, resource.kind().as_str(), &canonical_id, &adapter.binding.scope.hub_id, &adapter.binding.scope.account_id, resource.kind().as_str(), &canonical_id]).await?;
    Ok(!rows.is_empty())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MetadataRevision {
    before: Option<metadata::MetadataSource>,
    after: Option<metadata::MetadataSource>,
}

pub(super) fn project_revision(
    adapter: &StoreSyncAdapter,
    resource: &SyncResourceKey,
    known: bool,
    raw: &str,
) -> TalosResult<SyncExportFacts> {
    let revision: MetadataRevision = serde_json::from_str(raw).map_err(codec)?;
    let before = revision
        .before
        .map(|source| source.project(adapter, resource))
        .transpose()?;
    let after = revision
        .after
        .map(|source| source.project(adapter, resource))
        .transpose()?;
    let mut facts = SyncExportFacts::default();
    if let Some(baseline) = before.as_ref().filter(|_| !known) {
        facts.changes.push(SyncChange::Metadata {
            before: None,
            after: Box::new(baseline.clone()),
        });
    }
    if before != after {
        let change = match after {
            Some(after) => SyncChange::Metadata {
                before: before.map(Box::new),
                after: Box::new(after),
            },
            None => SyncChange::Delete,
        };
        facts.changes.push(change);
    }
    Ok(facts)
}

struct CaptureTransaction {
    tx: Box<dyn StorageTransaction>,
    cuts: Vec<(Arc<StoreSyncAdapter>, Vec<ResourceCut>)>,
}

#[async_trait]
impl StorageTransaction for CaptureTransaction {
    async fn query(&mut self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        self.tx.query(sql, params).await
    }
    async fn execute(&mut self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        self.tx.execute(sql, params).await
    }
    async fn rollback(self: Box<Self>) -> TalosResult<()> {
        self.tx.rollback().await
    }
    async fn commit(mut self: Box<Self>) -> TalosResult<()> {
        for (adapter, cuts) in &self.cuts {
            finish(adapter, &mut *self.tx, cuts).await?;
        }
        self.tx.commit().await?;
        for (adapter, _) in &self.cuts {
            adapter.wake_capture();
        }
        Ok(())
    }
}

pub(crate) async fn finish(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    cuts: &[ResourceCut],
) -> TalosResult<()> {
    for cut in cuts {
        let after = metadata::read_source(adapter, tx, &cut.resource).await?;
        if cut.metadata == after {
            continue;
        }
        let raw = serde_json::to_string(&MetadataRevision {
            before: cut.metadata.clone(),
            after,
        })
        .map_err(codec)?;
        tx.execute(
            "INSERT INTO resource_revisions(kind, resource_id, owner_user_id, execution_node_id, changes_json) VALUES (?, ?, ?, '', ?)",
            &[cut.resource.kind().as_str(), cut.resource.id(), &adapter.binding.scope.account_id, &raw],
        ).await?;
    }
    Ok(())
}

/// Typed SQLx Store commands borrow the same immediate transaction for capture.
pub(crate) struct SqliteCaptureTransaction {
    transaction: talos_storage::SqliteWriteTransaction,
    cuts: Vec<(Arc<StoreSyncAdapter>, Vec<ResourceCut>)>,
}
impl SqliteCaptureTransaction {
    pub(crate) async fn begin(
        storage: &talos_storage::SqliteStorage,
        bindings: Arc<StoreSyncBindings>,
        resources: Vec<SyncResourceKey>,
    ) -> TalosResult<Self> {
        let mut transaction = storage.write_transaction().await?;
        let mut cuts = Vec::new();
        for adapter in bindings.all() {
            let prepared = prepare(&adapter, &mut transaction, resources.clone()).await?;
            cuts.push((adapter, prepared));
        }
        Ok(Self { transaction, cuts })
    }
    pub(crate) async fn commit(mut self) -> TalosResult<()> {
        for (adapter, cuts) in &self.cuts {
            finish(adapter, &mut self.transaction, cuts).await?;
        }
        self.transaction.commit().await?;
        for (adapter, _) in &self.cuts {
            adapter.wake_capture();
        }
        Ok(())
    }
    pub(crate) async fn rollback(self) -> TalosResult<()> {
        self.transaction.rollback().await
    }
}
impl std::ops::Deref for SqliteCaptureTransaction {
    type Target = sqlx::SqliteConnection;
    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}
impl std::ops::DerefMut for SqliteCaptureTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.transaction
    }
}
