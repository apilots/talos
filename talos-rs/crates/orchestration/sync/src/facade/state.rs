use crate::SyncResourceSummary;
use talos_error::{TalosError, TalosResult};
use talos_sync_parser::SessionProjection;

use super::TalosSync;
use crate::{store, SyncBinding, SyncResourceKey};

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
        let rows = self.storage.query("SELECT seq FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ? AND seq <= ? LIMIT 1", &[&binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id(), &cut.to_string()]).await?;
        Ok(!rows.is_empty())
    }

    pub(crate) async fn resource_summary(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
    ) -> TalosResult<Option<SyncResourceSummary>> {
        let mut tx = self.storage.begin_read_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let summary = store::resource_summary(&mut *tx, &binding.scope, resource).await?;
        tx.commit().await?;
        Ok(summary)
    }
}

impl TalosSync {
    /// Populate retained documents gradually after an upgrade. The lookup
    /// only visits documents missing the projection.
    pub async fn backfill_session_projections(
        &self,
        binding: &SyncBinding,
        after: &str,
    ) -> TalosResult<String> {
        const LIMIT: usize = 4;
        let rows = self.storage.query(
            "SELECT resource_id FROM sync_documents WHERE hub_id=? AND account_id=? AND kind='session' AND session_projection_json IS NULL AND resource_id>? ORDER BY resource_id LIMIT ?",
            &[&binding.scope.hub_id, &binding.scope.account_id, after, &LIMIT.to_string()],
        ).await?;
        let mut cursor = String::new();
        for row in &rows {
            cursor = store::text(row, "resource_id")?;
            if let Err(error) = self.backfill_session_projection(binding, &cursor).await {
                tracing::warn!(session = %cursor, code = ?error.error_code(), "Backfill Session projection");
            }
        }
        if rows.len() < LIMIT {
            cursor.clear();
        }
        Ok(cursor)
    }

    async fn backfill_session_projection(
        &self,
        binding: &SyncBinding,
        session_id: &str,
    ) -> TalosResult<()> {
        let resource = SyncResourceKey::Session {
            session_id: session_id.into(),
        };
        let mut tx = self.storage.begin_read_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let record = store::document(&mut *tx, &binding.scope, &resource).await?;
        tx.commit().await?;
        let Some(record) = record.filter(|record| !record.has_session_projection) else {
            return Ok(());
        };
        let version = record.version.clone();
        let writer = self.writer_id;
        let account = binding.scope.account_id.clone();
        let snapshot = tokio::task::spawn_blocking(move || {
            let (replica, _) = Self::restore_replica(writer, &resource, Some(record))?;
            store::encode(&SessionProjection::from_changes(
                &crate::facade::replica_changes(&replica, &resource, &account)?,
            ))
        })
        .await
        .map_err(|error| {
            TalosError::internal_with_source("Prepare retained Session projection.", error)
        })??;
        let mut tx = self.storage.begin_background_transaction().await?;
        tx.execute(
            "UPDATE sync_documents SET session_projection_json=? WHERE hub_id=? AND account_id=? AND kind='session' AND resource_id=? AND session_projection_json IS NULL AND version_vector=unhex(?) AND generation=?",
            &[&snapshot, &binding.scope.hub_id, &binding.scope.account_id, session_id, &store::hex(&version.version_vector), &version.history_generation.to_string()],
        ).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Read the committed original Session projection in the caller's transaction.
    /// The bound Worker's display fields remain facts; this does not select a
    /// Worker, resolve receiver defaults, acquire execution or perform network I/O.
    pub async fn session_snapshot(
        &self,
        tx: &mut dyn talos_storage::StorageTransaction,
        binding: &SyncBinding,
        session_id: &str,
    ) -> TalosResult<Option<crate::SyncSessionSnapshot>> {
        Ok(self
            .session_projection(tx, binding, session_id)
            .await?
            .and_then(|projection| projection.snapshot))
    }

    async fn session_projection(
        &self,
        tx: &mut dyn talos_storage::StorageTransaction,
        binding: &SyncBinding,
        session_id: &str,
    ) -> TalosResult<Option<SessionProjection>> {
        self.require_binding(tx, binding).await?;
        let resource = SyncResourceKey::Session {
            session_id: session_id.into(),
        };
        let rows = tx.query(
            "SELECT session_projection_json FROM sync_documents WHERE hub_id=? AND account_id=? AND kind='session' AND resource_id=?",
            &[&binding.scope.hub_id, &binding.scope.account_id, session_id],
        ).await?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        if row.get_string("session_projection_json").is_some() {
            return store::json(row, "session_projection_json").map(Some);
        }
        // Upgraded documents can be read before the background materializer
        // has written this projection for the first time.
        let (document, _) = self.load_document(tx, &binding.scope, &resource).await?;
        Ok(Some(SessionProjection::from_changes(
            &crate::facade::replica_changes(&document, &resource, &binding.scope.account_id)?,
        )))
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
        Ok(self
            .session_projection(tx, binding, &identity.session_id)
            .await?
            .and_then(|projection| {
                projection
                    .active_turns
                    .into_iter()
                    .find(|active| active.identity.turn_id == identity.turn_id)
            }))
    }
}
