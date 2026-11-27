use std::sync::{Arc, RwLock};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;

use crate::sync::StoreSyncAdapter;

/// Optional authenticated export consumers attached to the original Stores.
/// A cloud database may serve several accounts. These associations select only
/// the owner of the explicit resource; they are not a Worker registry.
#[derive(Default)]
pub struct StoreCaptureBindings(RwLock<Vec<Arc<StoreSyncAdapter>>>);

impl StoreCaptureBindings {
    pub fn install(&self, adapter: Arc<StoreSyncAdapter>) -> TalosResult<()> {
        let mut bindings = self.0.write().expect("Store sync bindings lock");
        if let Some(existing) = bindings.iter().find(|existing| {
            existing.binding.source_user_id == adapter.binding.source_user_id
                || existing.binding.scope.account_id == adapter.binding.scope.account_id
        }) {
            if existing.binding.scope.account_id != adapter.binding.scope.account_id
                || existing.binding.source_user_id != adapter.binding.source_user_id
                || existing.binding.peer_id != adapter.binding.peer_id
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Source data is already associated with another synchronization account.",
                ));
            }
        }
        bindings.retain(|existing| {
            existing.binding.scope.account_id != adapter.binding.scope.account_id
                || existing.binding.source_user_id != adapter.binding.source_user_id
        });
        bindings.push(adapter);
        Ok(())
    }

    /// Select the authorized association using only the original Session row.
    pub(crate) async fn session(
        &self,
        tx: &mut dyn StorageTransaction,
        session_id: &str,
    ) -> TalosResult<Option<Arc<StoreSyncAdapter>>> {
        let bindings = self.0.read().expect("Store sync bindings lock").clone();
        let mut selected = None;
        for adapter in bindings {
            if crate::sync::metadata::session_row(&adapter, tx, session_id)
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

    pub(crate) fn wake_capture(&self) {
        for adapter in self.all() {
            adapter.wake_capture();
        }
    }
}
