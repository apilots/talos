use super::{codec, StoreSyncAdapter};
use crate::capture::MetadataRevision;
use talos_error::TalosResult;
use talos_storage::StorageTransaction;
use talos_sync::{SyncChange, SyncExportFacts, SyncResourceKey};

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
