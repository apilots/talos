//! Convert every retained replica before runtime assembly, without account admission.
use std::sync::Arc;

use semver::Version;
use talos_storage::SqliteStorage;
use talos_sync::{SyncOptions, TalosSync};

use crate::runner::{UpgradeContext, UpgradeFuture, UpgradeTask, UpgradeTaskDefinition};

pub(crate) const UPGRADE_ID: &str = "202611220001_sync_replicas_v2";

struct SyncReplicaUpgrade {
    definition: UpgradeTaskDefinition,
    storage: Arc<SqliteStorage>,
}
pub(crate) fn upgrade_task(storage: Arc<SqliteStorage>) -> Box<dyn UpgradeTask> {
    Box::new(SyncReplicaUpgrade {
        definition: UpgradeTaskDefinition {
            id: UPGRADE_ID,
            revision: 1,
            target_version: Version::new(0, 1, 18),
            priority: 300,
        },
        storage,
    })
}
impl UpgradeTask for SyncReplicaUpgrade {
    fn definition(&self) -> &UpgradeTaskDefinition {
        &self.definition
    }
    fn execute<'a>(&'a self, context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
        Box::pin(async move {
            let sync = TalosSync::open(
                self.storage.clone(),
                SyncOptions {
                    hub_endpoint: context.config.sync.hub_endpoint.clone(),
                },
            )
            .await?;
            sync.upgrade_local_replicas().await
        })
    }
    fn cleanup<'a>(&'a self, _context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
#[path = "sync_replicas_tests.rs"]
mod tests;
