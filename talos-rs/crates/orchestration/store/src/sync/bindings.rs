use std::sync::{Arc, RwLock};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use talos_sync::SyncResourceKey;

use super::StoreSyncAdapter;

/// Authenticated capture associations installed on the original Stores.
/// A cloud database may serve several accounts. These associations select only
/// the owner of the explicit resource; they are not a Worker registry.
#[derive(Default)]
pub struct StoreSyncBindings(RwLock<Vec<Arc<StoreSyncAdapter>>>);

impl StoreSyncBindings {
    pub fn install(&self, adapter: Arc<StoreSyncAdapter>) -> TalosResult<()> {
        let mut bindings = self.0.write().expect("Store sync bindings lock");
        if let Some(existing) = bindings.iter().find(|existing| {
            existing.binding.source_user_id == adapter.binding.source_user_id
                || existing.binding.scope.account_id == adapter.binding.scope.account_id
        }) {
            if existing.binding != adapter.binding {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Source data is already associated with another synchronization account.",
                ));
            }
        }
        bindings.retain(|existing| existing.binding != adapter.binding);
        bindings.push(adapter);
        Ok(())
    }

    pub(crate) async fn resource(
        &self,
        tx: &mut dyn StorageTransaction,
        resource: &SyncResourceKey,
    ) -> TalosResult<Option<Arc<StoreSyncAdapter>>> {
        let bindings = self.0.read().expect("Store sync bindings lock").clone();
        let mut selected = None;
        for adapter in bindings {
            if super::metadata::read(&adapter, tx, resource)
                .await?
                .is_some()
            {
                if selected.is_some() {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Resource has more than one synchronization association.",
                    ));
                }
                selected = Some(adapter);
            }
        }
        Ok(selected)
    }

    pub(crate) fn all(&self) -> Vec<Arc<StoreSyncAdapter>> {
        self.0.read().expect("Store sync bindings lock").clone()
    }
}
