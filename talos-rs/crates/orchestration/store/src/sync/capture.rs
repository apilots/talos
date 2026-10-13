use super::{metadata, timeline, StoreSyncAdapter, StoreSyncBindings};
use async_trait::async_trait;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_storage::{Row, StorageTransaction};
use talos_sync::{SyncCaptureParams, SyncChange, SyncResourceKey, SyncResourceSnapshot};

pub(crate) struct ResourceCut {
    resource: SyncResourceKey,
    before: Option<SyncResourceSnapshot>,
    tail: i64,
    generation: u64,
    active: Option<talos_sync::SyncActiveTurnMetadata>,
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
        let before = metadata::read(adapter, tx, &resource).await?;
        let generation = adapter.generation(tx, &resource).await?;
        let tail = timeline::tail(tx, &resource).await?;
        // Establish the complete pre-mutation baseline in this same read cut.
        // Keyset pages and bounded capture batches keep large histories intact.
        if before.is_some() {
            let canonical_id = match &resource {
                SyncResourceKey::Computer { computer_id } => adapter.computer(computer_id),
                _ => resource.id().to_owned(),
            };
            let rows = tx.query("SELECT 1 AS present FROM sync_documents WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? UNION ALL SELECT 1 FROM sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? LIMIT 1", &[&adapter.binding.scope.hub_id, &adapter.binding.scope.account_id, resource.kind().as_str(), &canonical_id, &adapter.binding.scope.hub_id, &adapter.binding.scope.account_id, resource.kind().as_str(), &canonical_id]).await?;
            if rows.is_empty() {
                append(
                    adapter,
                    tx,
                    &resource,
                    vec![SyncChange::Metadata {
                        before: None,
                        after: Box::new(before.clone().expect("existing baseline")),
                    }],
                )
                .await?;
                if let SyncResourceKey::Session { session_id } = &resource {
                    let facts = timeline::export(adapter, tx, session_id, 0).await?;
                    append(adapter, tx, &resource, facts).await?;
                }
            }
        }
        let active = if matches!(resource, SyncResourceKey::Session { .. }) {
            timeline::active_metadata(adapter, tx, resource.id()).await?
        } else {
            None
        };
        cuts.push(ResourceCut {
            resource,
            before,
            tail,
            generation,
            active,
        });
    }
    Ok(cuts)
}

async fn append(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    resource: &SyncResourceKey,
    changes: Vec<SyncChange>,
) -> TalosResult<()> {
    if changes.is_empty() {
        return Ok(());
    }
    capture(adapter, tx, resource, changes).await
}

async fn capture(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    resource: &SyncResourceKey,
    changes: Vec<SyncChange>,
) -> TalosResult<()> {
    adapter
        .sync
        .capture(
            tx,
            SyncCaptureParams {
                binding: adapter.binding.clone(),
                resource: resource.clone(),
                changes,
                created_at: chrono::Utc::now().timestamp(),
            },
        )
        .await
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
            adapter.committed();
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
        let after = metadata::read(adapter, tx, &cut.resource).await?;
        let mut changes = Vec::new();
        match (cut.before.as_ref(), after) {
            (Some(_), None) => changes.push(SyncChange::Delete),
            (before, Some(after)) => {
                if before != Some(&after) {
                    changes.push(SyncChange::Metadata {
                        before: before.cloned().map(Box::new),
                        after: Box::new(after),
                    });
                }
                if let SyncResourceKey::Session { session_id } = &cut.resource {
                    let tail = timeline::tail(tx, &cut.resource).await?;
                    // Explicit clear is tracked by the original persisted generation.
                    let generation = adapter.generation(tx, &cut.resource).await?;
                    if generation > cut.generation {
                        changes.extend(timeline::export(adapter, tx, session_id, 0).await?);
                    } else {
                        changes.extend(
                            timeline::export(adapter, tx, session_id, cut.tail.min(tail)).await?,
                        );
                    }
                }
            }
            (None, None) => {}
        }
        changes.retain(|change| !matches!(change, SyncChange::ActiveTurnMetadata(active) if cut.active.as_ref() == Some(active)));
        append(adapter, tx, &cut.resource, changes).await?;
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
        pool: &sqlx::SqlitePool,
        bindings: Arc<StoreSyncBindings>,
        resources: Vec<SyncResourceKey>,
    ) -> TalosResult<Self> {
        let mut transaction = talos_storage::SqliteWriteTransaction::begin(pool).await?;
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
            adapter.committed();
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
