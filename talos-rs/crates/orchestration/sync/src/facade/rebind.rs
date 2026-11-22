//! Move an account's retained replicas to a fresh Hub without rebuilding origins.
use talos_error::{ErrorCode, TalosError, TalosResult};

use super::TalosSync;
use crate::{store, SyncBinding, SyncResourceKey};

impl TalosSync {
    pub(super) async fn replace_binding(&self, next: &SyncBinding) -> TalosResult<bool> {
        let bindings = self.saved_bindings().await?;
        let previous: Vec<_> = bindings
            .into_iter()
            .filter(|binding| {
                binding.scope.account_id == next.scope.account_id
                    && binding.source_user_id == next.source_user_id
                    && binding.scope != next.scope
            })
            .collect();
        if previous.len() > 1 {
            return Err(conflict(
                "The source has more than one previous Hub binding.",
            ));
        }
        let Some(previous) = previous.first() else {
            return Ok(false);
        };
        if previous
            .computers
            .iter()
            .any(|alias| !next.computers.contains(alias))
        {
            return Err(conflict(
                "A new Hub must retain the original Computer associations.",
            ));
        }
        self.upgrade_binding(previous).await?;
        for _ in 0..3 {
            let resources = self.storage.query(
                "SELECT kind, resource_id FROM sync_documents WHERE hub_id=?1 AND account_id=?2
                 UNION SELECT kind, resource_id FROM visible_sync_outbox WHERE hub_id=?1 AND account_id=?2",
                &[&previous.scope.hub_id, &previous.scope.account_id],
            ).await?;
            for row in &resources {
                let key: SyncResourceKey = talos_sync_types::SyncResourceKey::new(
                    store::text(row, "kind")?,
                    store::text(row, "resource_id")?,
                )
                .try_into()?;
                // Freeze pending local operations, retaining their original Loro identities.
                self.flush_cut(previous, &key).await?;
            }
            let mut tx = self.storage.begin_background_transaction().await?;
            self.require_binding(&mut *tx, previous).await?;
            if !tx.query(
                "SELECT 1 FROM visible_sync_outbox WHERE hub_id=? AND account_id=? AND exported_version_json IS NULL LIMIT 1",
                &[&previous.scope.hub_id, &previous.scope.account_id],
            ).await?.is_empty() {
                tx.rollback().await?;
                continue;
            }
            if !tx
                .query(
                    "SELECT 1 FROM sync_documents WHERE hub_id=? AND account_id=? LIMIT 1",
                    &[&next.scope.hub_id, &next.scope.account_id],
                )
                .await?
                .is_empty()
            {
                return Err(conflict(
                    "The replacement Hub scope already has retained replicas.",
                ));
            }
            store::save_binding(&mut *tx, next).await?;
            store::copy_checkpoint_scope(&mut *tx, &previous.scope, &next.scope).await?;
            let capture = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO sync_capture_batches(id,state) VALUES (?,'published')",
                &[&capture],
            )
            .await?;
            tx.execute(
                "INSERT INTO sync_event_map(hub_id,account_id,session_id,generation,stable_event_key,local_event_id,local_event_seq,capture_id)
                 SELECT DISTINCT ?1,account_id,session_id,generation,stable_event_key,local_event_id,local_event_seq,?4
                 FROM visible_sync_event_map WHERE hub_id=?2 AND account_id=?3",
                &[&next.scope.hub_id, &previous.scope.hub_id, &next.scope.account_id, &capture],
            ).await?;
            let resources = tx
                .query(
                    "SELECT kind,resource_id FROM sync_documents WHERE hub_id=? AND account_id=?",
                    &[&next.scope.hub_id, &next.scope.account_id],
                )
                .await?;
            for row in resources {
                let key: SyncResourceKey = talos_sync_types::SyncResourceKey::new(
                    store::text(&row, "kind")?,
                    store::text(&row, "resource_id")?,
                )
                .try_into()?;
                // A fresh account feed must ACK the complete retained replica, including deletions.
                store::append(&mut *tx, next, &key, "[]", store::timestamp()?, &capture).await?;
                tx.execute(
                    "INSERT INTO sync_resource_capture(hub_id,account_id,kind,resource_id,revision,outbox_sequence)
                     SELECT ?1,account_id,kind,resource_id,revision,
                        (SELECT MAX(seq) FROM visible_sync_outbox WHERE hub_id=?1 AND account_id=?3 AND kind=?4 AND resource_id=?5)
                     FROM sync_resource_capture WHERE hub_id=?2 AND account_id=?3 AND kind=?4 AND resource_id=?5",
                    &[&next.scope.hub_id, &previous.scope.hub_id, &next.scope.account_id, key.kind().as_str(), key.id()],
                ).await?;
            }
            // All committed pending operations are now in the retained baseline and
            // require a new Hub ACK. Remove old queue rows before retiring its binding.
            tx.execute(
                "DELETE FROM sync_outbox WHERE hub_id=? AND account_id=? AND source_user_id=?",
                &[
                    &previous.scope.hub_id,
                    &next.scope.account_id,
                    &next.source_user_id,
                ],
            )
            .await?;
            tx.execute(
                "DELETE FROM sync_bindings WHERE hub_id=? AND account_id=? AND source_user_id=?",
                &[
                    &previous.scope.hub_id,
                    &next.scope.account_id,
                    &next.source_user_id,
                ],
            )
            .await?;
            tx.commit().await?;
            return Ok(true);
        }
        Err(conflict(
            "Local synchronization facts changed while replacing the Hub binding.",
        ))
    }
}

fn conflict(message: &str) -> TalosError {
    TalosError::new(ErrorCode::Conflict, message)
}
