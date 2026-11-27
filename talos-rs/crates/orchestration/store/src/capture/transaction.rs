use super::{metadata, MetadataRevision, MetadataSource, ResourceKey, StoreCaptureBindings};
use async_trait::async_trait;
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
pub(crate) struct ResourceCut {
    resource: ResourceKey,
    metadata: Option<MetadataSource>,
}
pub(crate) async fn prepare(
    tx: &mut dyn StorageTransaction,
    resources: Vec<ResourceKey>,
) -> TalosResult<Vec<ResourceCut>> {
    let mut cuts = Vec::new();
    for resource in resources {
        let metadata = metadata::read(tx, &resource).await?;
        cuts.push(ResourceCut { resource, metadata });
    }
    Ok(cuts)
}
pub(crate) async fn finish(
    tx: &mut dyn StorageTransaction,
    cuts: &[ResourceCut],
) -> TalosResult<()> {
    for cut in cuts {
        let after = metadata::read(tx, &cut.resource).await?;
        if cut.metadata == after {
            continue;
        }
        let source = after
            .as_ref()
            .or(cut.metadata.as_ref())
            .expect("changed resource exists");
        let row = source.fields.row();
        let owner = super::text(&row, "owner_user_id")?;
        let node = row.get_string("execution_node_id").unwrap_or_default();
        let raw = serde_json::to_string(&MetadataRevision {
            before: cut.metadata.clone(),
            after,
        })
        .map_err(|error| TalosError::internal_with_source("Encode resource revision.", error))?;
        tx.execute("INSERT INTO resource_revisions(kind,resource_id,owner_user_id,execution_node_id,changes_json) VALUES (?,?,?,?,?)", &[cut.resource.kind(),cut.resource.id(),&owner,&node,&raw]).await?;
    }
    Ok(())
}
pub(crate) async fn transaction(
    mut tx: Box<dyn StorageTransaction>,
    bindings: Arc<StoreCaptureBindings>,
    resources: Vec<ResourceKey>,
) -> TalosResult<Box<dyn StorageTransaction>> {
    let cuts = prepare(&mut *tx, resources).await?;
    Ok(Box::new(CaptureTransaction { tx, bindings, cuts }))
}
struct CaptureTransaction {
    tx: Box<dyn StorageTransaction>,
    bindings: Arc<StoreCaptureBindings>,
    cuts: Vec<ResourceCut>,
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
        if let Err(error) = finish(&mut *self.tx, &self.cuts).await {
            self.tx.rollback().await?;
            return Err(error);
        }
        self.tx.commit().await?;
        self.bindings.wake_capture();
        Ok(())
    }
}
pub(crate) struct SqliteCaptureTransaction {
    transaction: talos_storage::SqliteWriteTransaction,
    bindings: Arc<StoreCaptureBindings>,
    cuts: Vec<ResourceCut>,
}
impl SqliteCaptureTransaction {
    pub(crate) async fn begin(
        storage: &talos_storage::SqliteStorage,
        bindings: Arc<StoreCaptureBindings>,
        resources: Vec<ResourceKey>,
    ) -> TalosResult<Self> {
        let mut transaction = storage.write_transaction().await?;
        let cuts = prepare(&mut transaction, resources).await?;
        Ok(Self {
            transaction,
            bindings,
            cuts,
        })
    }
    pub(crate) async fn commit(mut self) -> TalosResult<()> {
        if let Err(error) = finish(&mut self.transaction, &self.cuts).await {
            self.transaction.rollback().await?;
            return Err(error);
        }
        self.transaction.commit().await?;
        self.bindings.wake_capture();
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
