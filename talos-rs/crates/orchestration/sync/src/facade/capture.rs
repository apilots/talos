use crate::ResourceKind;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;

use super::{sync_error, TalosSync};
use crate::{
    store::{
        self,
        staged_write::{Kind, StagedWrite},
    },
    PreparedSyncCapture, SyncBinding, SyncCaptureParams, SyncEventMapping,
};

const MAX_BATCH_BYTES: usize = 8 * 1024 * 1024;
const WRITE_BATCH_BYTES: usize = 256 * 1024;
const WRITE_BATCH_ROWS: usize = 128;

const EMBEDDED_CAPTURE_ELIGIBLE: &str = "
    scope.embedded_computer <> '' AND (
        NOT EXISTS (
            SELECT 1 FROM local_computer_retirements
            WHERE node_id = scope.embedded_computer AND owner_user_id = scope.account_id
        ) OR EXISTS (
            SELECT 1 FROM local_computer_retirement_resources
            WHERE node_id = scope.embedded_computer
              AND kind = resource.kind AND resource_id = resource.resource_id
        )
    )";

fn embedded_computer(binding: &SyncBinding) -> &str {
    binding
        .computers
        .iter()
        .find(|alias| alias.source_node_id == "embedded")
        .map_or("", |alias| alias.computer_id.as_str())
}

impl TalosSync {
    /// Check the binding's existing capture eligibility for an embedded resource.
    pub async fn captures_embedded_resource(
        tx: &mut dyn StorageTransaction,
        binding: &SyncBinding,
        kind: ResourceKind,
        id: &str,
    ) -> TalosResult<bool> {
        let computer = embedded_computer(binding);
        if computer.is_empty() {
            return Ok(false);
        }
        let rows = tx
            .query(
                &format!(
                    "WITH scope(account_id, embedded_computer) AS (VALUES (?, ?)),
                          resource(kind, resource_id) AS (VALUES (?, ?))
                     SELECT 1 FROM scope CROSS JOIN resource WHERE {EMBEDDED_CAPTURE_ELIGIBLE}"
                ),
                &[&binding.scope.account_id, computer, kind.as_str(), id],
            )
            .await?;
        Ok(!rows.is_empty())
    }

    /// Count eligible uncollected revisions and published outbox entries in one read.
    pub async fn pending_changes(&self, binding: &SyncBinding) -> TalosResult<u64> {
        let rows = self.storage.query(
            &format!(
                "WITH scope(hub_id, account_id, source_user_id, embedded_computer) AS (VALUES (?, ?, ?, ?))
                 SELECT
                    (SELECT COUNT(*) FROM visible_sync_outbox o
                     WHERE o.hub_id = scope.hub_id AND o.account_id = scope.account_id)
                  + (SELECT COUNT(*) FROM resource_revisions resource
                     WHERE resource.owner_user_id IN (scope.account_id, scope.source_user_id)
                       AND NOT EXISTS (
                           SELECT 1 FROM sync_resource_capture c
                           WHERE c.hub_id = scope.hub_id AND c.account_id = scope.account_id
                             AND c.kind = resource.kind AND c.resource_id = resource.resource_id
                             AND c.revision >= resource.revision
                       )
                       AND (resource.execution_node_id <> 'embedded' OR ({EMBEDDED_CAPTURE_ELIGIBLE})))
                  AS count FROM scope"
            ),
            &[
                &binding.scope.hub_id,
                &binding.scope.account_id,
                &binding.source_user_id,
                embedded_computer(binding),
            ],
        ).await?;
        rows.first()
            .and_then(|row| row.get_i64("count"))
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| TalosError::internal("Invalid synchronization pending count."))
    }

    /// Project and encode source facts without acquiring a database writer.
    pub fn prepare_capture(params: SyncCaptureParams) -> TalosResult<PreparedSyncCapture> {
        let resource = talos_sync_parser::resource(&params.binding, &params.resource);
        resource.validate().map_err(sync_error)?;
        let mut batches = Vec::new();
        let mut batch = Vec::new();
        let mut bytes = 0;
        for change in params.changes {
            let Some(change) =
                talos_sync_parser::encode_change(&params.binding, &resource, change)?
            else {
                continue;
            };
            let size = store::encode(&change)?.len();
            if size > MAX_BATCH_BYTES {
                return Err(TalosError::new(
                    ErrorCode::UploadTooLarge,
                    "A synchronization fact exceeds the limit.",
                ));
            }
            if !batch.is_empty()
                && (bytes + size > WRITE_BATCH_BYTES || batch.len() >= WRITE_BATCH_ROWS)
            {
                batches.push(store::encode(&batch)?);
                batch.clear();
                bytes = 0;
            }
            bytes += size;
            batch.push(change);
        }
        if !batch.is_empty() {
            batches.push(store::encode(&batch)?);
        }
        Ok(PreparedSyncCapture {
            binding: params.binding,
            resource,
            batches,
            created_at: params.created_at,
            stage: None,
        })
    }

    /// Persist large captures in unlisted outbox batches before publishing
    /// their source checkpoint. Small captures stay in the original transaction.
    pub async fn stage_capture(
        &self,
        mut prepared: PreparedSyncCapture,
        mappings: Vec<SyncEventMapping>,
    ) -> TalosResult<(PreparedSyncCapture, Vec<SyncEventMapping>)> {
        if prepared.batches.len() <= 1 && mappings.len() <= WRITE_BATCH_ROWS {
            return Ok((prepared, mappings));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut tx = self.storage.begin_background_transaction().await?;
        self.require_binding(&mut *tx, &prepared.binding).await?;
        tx.execute(
            "INSERT INTO sync_capture_batches(id,state) VALUES (?,'staged')",
            &[&id],
        )
        .await?;
        tx.commit().await?;
        let staged = StagedWrite {
            id,
            storage: self.storage.clone(),
            kind: Kind::Capture,
        };
        for batch in std::mem::take(&mut prepared.batches) {
            let mut tx = self.storage.begin_background_transaction().await?;
            staged.require_staged(&mut *tx).await?;
            store::append(
                &mut *tx,
                &prepared.binding,
                &prepared.resource,
                &batch,
                prepared.created_at,
                &staged.id,
            )
            .await?;
            tx.commit().await?;
            tokio::task::yield_now().await;
        }
        let mut mappings = mappings.into_iter();
        loop {
            let batch: Vec<_> = mappings.by_ref().take(WRITE_BATCH_ROWS).collect();
            if batch.is_empty() {
                break;
            }
            let mut tx = self.storage.begin_background_transaction().await?;
            staged.require_staged(&mut *tx).await?;
            for mapping in batch {
                self.map_event_for_capture(&mut *tx, mapping, &staged.id)
                    .await?;
            }
            tx.commit().await?;
            tokio::task::yield_now().await;
        }
        prepared.stage = Some(staged);
        Ok((prepared, Vec::new()))
    }

    pub async fn reclaim_capture_batch(&self) -> TalosResult<bool> {
        let rows = self
            .storage
            .query(
                "SELECT c.id FROM sync_capture_batches c
             WHERE c.state='retired' OR (c.state='published'
               AND NOT EXISTS (SELECT 1 FROM sync_outbox o WHERE o.capture_id=c.id)
               AND NOT EXISTS (SELECT 1 FROM sync_event_map m WHERE m.capture_id=c.id))
             ORDER BY c.id LIMIT 1",
                &[],
            )
            .await?;
        let Some(row) = rows.first() else {
            return Ok(false);
        };
        let id = store::text(row, "id")?;
        let mut tx = self.storage.begin_background_transaction().await?;
        let removed = tx
            .execute(
                "DELETE FROM sync_outbox WHERE capture_id=?1 AND seq=(
                SELECT MIN(seq) FROM sync_outbox WHERE capture_id=?1
            )",
                &[&id],
            )
            .await?;
        if removed == 0 {
            let mappings = tx.execute(
                "DELETE FROM sync_event_map WHERE capture_id=?1 AND local_event_seq IN (
                    SELECT local_event_seq FROM sync_event_map WHERE capture_id=?1 ORDER BY local_event_seq LIMIT ?2
                )",
                &[&id, &WRITE_BATCH_ROWS.to_string()],
            ).await?;
            if mappings < WRITE_BATCH_ROWS as u64 {
                tx.execute("DELETE FROM sync_capture_batches WHERE id=?", &[&id])
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Append a prepared cut under the current binding. The source owner is
    /// responsible for validating its cut and committing source metadata here.
    pub async fn append_capture(
        &self,
        tx: &mut dyn StorageTransaction,
        prepared: PreparedSyncCapture,
    ) -> TalosResult<()> {
        self.require_binding(tx, &prepared.binding).await?;
        Self::write_capture(tx, prepared).await
    }

    /// Capture canonical facts in the caller's transaction.
    pub async fn capture(
        &self,
        tx: &mut dyn StorageTransaction,
        params: SyncCaptureParams,
    ) -> TalosResult<()> {
        self.require_binding(tx, &params.binding).await?;
        Self::write_capture(tx, Self::prepare_capture(params)?).await
    }

    async fn write_capture(
        tx: &mut dyn StorageTransaction,
        prepared: PreparedSyncCapture,
    ) -> TalosResult<()> {
        if let Some(staged) = prepared.stage {
            return staged.publish(tx).await;
        }
        for batch in prepared.batches {
            store::append(
                tx,
                &prepared.binding,
                &prepared.resource,
                &batch,
                prepared.created_at,
                "",
            )
            .await?;
        }
        Ok(())
    }
}
