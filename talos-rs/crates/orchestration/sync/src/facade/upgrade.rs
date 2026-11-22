//! Persist the released replica conversion once, outside foreground Store reads.
use super::TalosSync;
use crate::{store, SyncBinding, SyncChange, SyncResourceKey, SyncVersion};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_parser::{upgrade_v1_snapshot, SessionProjection};
use talos_sync_sdk::{version_equal, SyncReplica};

impl TalosSync {
    pub(super) async fn upgrade_binding(&self, binding: &SyncBinding) -> TalosResult<()> {
        let mut after = (String::new(), String::new());
        loop {
            let rows = self.storage.query(
                "SELECT kind,resource_id FROM sync_documents WHERE hub_id=? AND account_id=? AND replica_schema_version=1 AND (kind,resource_id)>(?,?) ORDER BY kind,resource_id LIMIT 1",
                &[&binding.scope.hub_id, &binding.scope.account_id, &after.0, &after.1],
            ).await?;
            let Some(row) = rows.first() else { break };
            after = (store::text(row, "kind")?, store::text(row, "resource_id")?);
            let key: SyncResourceKey =
                talos_sync_types::SyncResourceKey::new(&after.0, &after.1).try_into()?;
            self.upgrade_document(binding, &key).await?;
        }
        self.upgrade_outbox(binding).await
    }
    async fn upgrade_document(
        &self,
        binding: &SyncBinding,
        key: &SyncResourceKey,
    ) -> TalosResult<()> {
        for _ in 0..3 {
            let mut read = self.storage.begin_read_transaction().await?;
            let Some(record) = store::document(&mut *read, &binding.scope, key).await? else {
                return Ok(());
            };
            read.commit().await?;
            if record.schema_version == 2 {
                return Ok(());
            }
            let base = (record.version.clone(), record.acknowledged.clone());
            let owned_binding = binding.clone();
            let resource = key.clone();
            let writer = self.writer_id;
            let candidate = tokio::task::spawn_blocking(move || {
                let Some(snapshot) = upgrade_v1_snapshot(
                    &owned_binding,
                    &resource,
                    writer,
                    &record.snapshot,
                    &record.version,
                )?
                else {
                    return Ok(None);
                };
                let replica = SyncReplica::from_snapshot(resource.wire(), &snapshot)
                    .map_err(super::sync_error)?;
                let summary = replica.summary().map_err(super::sync_error)?;
                if summary.version.history_generation != record.version.history_generation {
                    return Err(changed());
                }
                let summary = crate::SyncResourceSummary {
                    resource: resource.clone(),
                    computer_id: replica
                        .header()
                        .map_err(super::sync_error)?
                        .and_then(|header| header.related)
                        .map(|key| key.id().to_owned()),
                    version: summary.version,
                    deleted: summary.deleted,
                };
                let projection = matches!(resource, SyncResourceKey::Session { .. })
                    .then(|| {
                        super::replica_changes(&replica, &resource, &owned_binding.scope.account_id)
                            .and_then(|changes| {
                                store::encode(&SessionProjection::from_changes(&changes))
                            })
                    })
                    .transpose()?;
                Ok::<_, TalosError>(Some((snapshot, summary, projection)))
            })
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Prepare a retained replica upgrade.", error)
            })??;
            let prepared = match candidate {
                Some((snapshot, summary, projection)) => Some((
                    store::snapshot::Snapshot::prepare(self.storage.clone(), snapshot).await?,
                    summary,
                    projection,
                )),
                None => None,
            };
            let mut tx = self.storage.begin_background_transaction().await?;
            let current = store::document_checkpoint(&mut *tx, &binding.scope, key).await?;
            let unchanged = match current {
                Some((version, ack)) => {
                    version_equal(&version, &base.0).map_err(super::sync_error)?
                        && version_equal(&ack, &base.1).map_err(super::sync_error)?
                }
                None => false,
            };
            if !unchanged {
                tx.rollback().await?;
                continue;
            }
            if let Some((snapshot, summary, projection)) = prepared {
                store::save_document(
                    &mut *tx,
                    &binding.scope,
                    &summary,
                    &snapshot,
                    &SyncVersion::default(),
                    projection.as_deref(),
                )
                .await?;
                // Already materialized facts must never be replayed (especially resets/deletes).
                // The complete converted checkpoint is a conservative superset of each old cut.
                let proof = store::encode(&summary.version)?;
                tx.execute("UPDATE sync_outbox SET exported_version_json=? WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=? AND exported_version_json IS NOT NULL", &[&proof,&binding.scope.hub_id,&binding.scope.account_id,key.kind().as_str(),key.id()]).await?;
                tx.execute("UPDATE sync_resource_capture SET exported_version_json=? WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=? AND exported_version_json IS NOT NULL", &[&proof,&binding.scope.hub_id,&binding.scope.account_id,key.kind().as_str(),key.id()]).await?;
            } else {
                tx.execute("UPDATE sync_documents SET replica_schema_version=2 WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=?", &[&binding.scope.hub_id,&binding.scope.account_id,key.kind().as_str(),key.id()]).await?;
            }
            tx.commit().await?;
            return Ok(());
        }
        Err(changed())
    }
    async fn upgrade_outbox(&self, binding: &SyncBinding) -> TalosResult<()> {
        let through = self.storage.query("SELECT COALESCE(MAX(seq),0) AS seq FROM visible_sync_outbox WHERE hub_id=? AND account_id=?", &[&binding.scope.hub_id,&binding.scope.account_id]).await?;
        let through = through[0].get_i64("seq").ok_or_else(changed)?;
        let mut after = 0_i64;
        loop {
            let rows = self.storage.query("SELECT seq,kind,resource_id,changes_json FROM visible_sync_outbox WHERE hub_id=? AND account_id=? AND seq>? AND seq<=? ORDER BY seq LIMIT 128", &[&binding.scope.hub_id,&binding.scope.account_id,&after.to_string(),&through.to_string()]).await?;
            if rows.is_empty() {
                return Ok(());
            }
            let mut updates = Vec::new();
            for row in rows {
                after = row.get_i64("seq").ok_or_else(changed)?;
                let encoded = store::text(&row, "changes_json")?;
                if serde_json::from_str::<Vec<talos_sync_types::SyncChange>>(&encoded).is_ok() {
                    continue;
                }
                let native: Vec<SyncChange> =
                    serde_json::from_str(&encoded).map_err(talos_sync_parser::codec_error)?;
                let resource: SyncResourceKey = talos_sync_types::SyncResourceKey::new(
                    store::text(&row, "kind")?,
                    store::text(&row, "resource_id")?,
                )
                .try_into()?;
                let wire = native
                    .into_iter()
                    .map(|change| talos_sync_parser::encode_change(binding, &resource, change))
                    .collect::<TalosResult<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                updates.push((after, encoded, store::encode(&wire)?));
            }
            if updates.is_empty() {
                continue;
            }
            let mut tx = self.storage.begin_background_transaction().await?;
            for (seq, previous, next) in updates {
                tx.execute(
                    "UPDATE sync_outbox SET changes_json=? WHERE seq=? AND changes_json=?",
                    &[&next, &seq.to_string(), &previous],
                )
                .await?;
            }
            tx.commit().await?;
        }
    }
}
fn changed() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Retained synchronization state changed during upgrade.",
    )
}
