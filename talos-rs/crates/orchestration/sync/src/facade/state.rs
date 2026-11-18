use talos_error::{TalosError, TalosResult};
use talos_sync_types::SyncResourceSummary;

use super::TalosSync;
use crate::{facade::sync_error, store, SyncBinding, SyncResourceKey};

impl TalosSync {
    /// Retire only the specified local capture alias. The product transaction must
    /// first freeze existing resources to their canonical Computer UUID. Account,
    /// peer, source ownership and historical document bindings never change.
    pub async fn retire_computer_alias(
        &self,
        tx: &mut dyn talos_storage::StorageTransaction,
        binding: &crate::SyncBinding,
        computer_id: &str,
    ) -> TalosResult<crate::SyncBinding> {
        self.require_binding(tx, binding).await?;
        let mut next = binding.clone();
        next.computers
            .retain(|alias| alias.computer_id != computer_id);
        let computers = crate::store::encode(&next.computers)?;
        tx.execute("UPDATE sync_bindings SET computers_json = ? WHERE hub_id = ? AND account_id = ? AND source_user_id = ?", &[&computers, &binding.scope.hub_id, &binding.scope.account_id, &binding.source_user_id]).await?;
        Ok(next)
    }

    /// An account without a saved choice follows authenticated login by default.
    /// Deployment policy is enforced by the management host before starting transport.
    pub async fn enabled(&self, account_id: &str) -> TalosResult<bool> {
        let rows = self
            .storage
            .query(
                "SELECT enabled FROM sync_account_preferences WHERE account_id = ?",
                &[account_id],
            )
            .await?;
        let enabled = rows.first().and_then(|row| row.get_i64("enabled"));
        match enabled {
            None if rows.is_empty() => Ok(true),
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(TalosError::internal(
                "Invalid synchronization enabled state.",
            )),
        }
    }

    pub(crate) async fn has_pending_through(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        cut: i64,
    ) -> TalosResult<bool> {
        let rows = self.storage.query("SELECT seq FROM sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? AND seq <= ? LIMIT 1", &[&binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id(), &cut.to_string()]).await?;
        Ok(!rows.is_empty())
    }

    pub(crate) async fn resource_summary(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
    ) -> TalosResult<Option<SyncResourceSummary>> {
        let mut tx = self.storage.begin_read_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let summary = if store::document(&mut *tx, &binding.scope, resource)
            .await?
            .is_some()
        {
            let (document, _) = self
                .load_document(&mut *tx, &binding.scope, resource)
                .await?;
            Some(document.summary().map_err(sync_error)?)
        } else {
            None
        };
        tx.commit().await?;
        Ok(summary)
    }
}

impl TalosSync {
    /// Read the committed original Session projection in the caller's transaction.
    /// The bound Worker's display fields remain facts; this does not select a
    /// Worker, resolve receiver defaults, acquire execution or perform network I/O.
    pub async fn session_snapshot(
        &self,
        tx: &mut dyn talos_storage::StorageTransaction,
        binding: &SyncBinding,
        session_id: &str,
    ) -> TalosResult<Option<crate::SyncSessionSnapshot>> {
        self.require_binding(tx, binding).await?;
        let resource = SyncResourceKey::Session {
            session_id: session_id.into(),
        };
        if store::document(tx, &binding.scope, &resource)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        let (document, _) = self.load_document(tx, &binding.scope, &resource).await?;
        for change in document.changes().map_err(sync_error)? {
            if let talos_sync_types::SyncChange::Metadata { after, .. } = change {
                if let talos_sync_types::SyncResourceSnapshot::Session(session) = *after {
                    return crate::projection::transcode(&session).map(Some);
                }
            }
        }
        Ok(None)
    }
}

impl TalosSync {
    /// Read the minimal display fact for an exact Turn. This grants no execution.
    pub async fn active_turn_metadata(
        &self,
        tx: &mut dyn talos_storage::StorageTransaction,
        binding: &SyncBinding,
        identity: &talos_types::AgentTurnRef,
    ) -> TalosResult<Option<crate::SyncActiveTurnMetadata>> {
        self.require_binding(tx, binding).await?;
        let resource = SyncResourceKey::Session {
            session_id: identity.session_id.clone(),
        };
        if store::document(tx, &binding.scope, &resource)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        let (document, _) = self.load_document(tx, &binding.scope, &resource).await?;
        for change in document.changes().map_err(sync_error)? {
            if let talos_sync_types::SyncChange::ActiveTurnMetadata(active) = change {
                if active.identity.turn_id == identity.turn_id {
                    return crate::projection::transcode(&active).map(Some);
                }
            }
        }
        Ok(None)
    }
}
