//! Persist the released replica conversion once, outside foreground Store reads.
use super::TalosSync;
use crate::{store, SyncBinding, SyncChange, SyncResourceKey};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_parser::{upgrade_v1_snapshot, SessionProjection};
use talos_sync_sdk::{version_equal, SyncReplica};

impl TalosSync {
    /// Verify persisted replicas after the host's versioned startup task completes.
    /// A database restored with an unrelated completed ledger fails explicitly.
    pub async fn require_upgraded_replicas(
        storage: &dyn talos_storage::Storage,
    ) -> TalosResult<()> {
        if !storage
            .query(
                "SELECT 1 FROM sync_documents WHERE replica_schema_version<>2 LIMIT 1",
                &[],
            )
            .await?
            .is_empty()
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Retained synchronization replicas require the startup upgrade task.",
            ));
        }
        let mut after = 0_i64;
        loop {
            let rows = storage.query(
                "SELECT seq,changes_json FROM visible_sync_outbox WHERE seq>? ORDER BY seq LIMIT 16",
                &[&after.to_string()],
            ).await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                after = row.get_i64("seq").ok_or_else(changed)?;
                serde_json::from_str::<Vec<talos_sync_types::SyncChange>>(&store::text(
                    &row,
                    "changes_json",
                )?)
                .map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::Conflict,
                        "Retained synchronization outbox requires the startup upgrade task.",
                        error,
                    )
                })?;
            }
        }
        Ok(())
    }
    /// Upgrade all persisted scopes before starting Services, capture workers or transports.
    /// This trusted startup operation does not authenticate or connect any account.
    pub async fn upgrade_local_replicas(&self) -> TalosResult<()> {
        let bindings = self.saved_bindings().await?;
        let scopes = self.storage.query(
            "SELECT hub_id,account_id FROM sync_documents UNION SELECT hub_id,account_id FROM sync_outbox ORDER BY hub_id,account_id", &[],
        ).await?;
        for row in scopes {
            let scope = crate::SyncScope {
                hub_id: store::text(&row, "hub_id")?,
                account_id: store::text(&row, "account_id")?,
            };
            let binding = bindings
                .iter()
                .find(|binding| binding.scope == scope)
                .cloned()
                .unwrap_or_else(|| SyncBinding {
                    source_user_id: scope.account_id.clone(),
                    scope,
                    peer_id: self.peer_id().into(),
                    computers: vec![],
                });
            self.upgrade_scope(&binding).await?;
        }
        Ok(())
    }
    async fn upgrade_scope(&self, binding: &SyncBinding) -> TalosResult<()> {
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
            let candidate = tokio::task::spawn_blocking(move || {
                let Some(snapshot) = upgrade_v1_snapshot(
                    &owned_binding,
                    &resource,
                    &record.snapshot,
                    &record.version,
                )?
                else {
                    let replica = SyncReplica::from_snapshot(resource.wire(), &record.snapshot)
                        .map_err(super::sync_error)?;
                    if !version_equal(
                        &replica.version().map_err(super::sync_error)?,
                        &record.version,
                    )
                    .map_err(super::sync_error)?
                    {
                        return Err(changed());
                    }
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
                    &base.1,
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
            // The old feed cannot describe the new protocol envelope. Catalog
            // recovery downloads only the causal operations missing locally.
            tx.execute(
                "UPDATE sync_bindings SET download_cursor=NULL WHERE hub_id=? AND account_id=?",
                &[&binding.scope.hub_id, &binding.scope.account_id],
            )
            .await?;
            if prepared_header_marker(&mut *tx, binding, key).await? {
                tracing::debug!(account_id=%binding.scope.account_id, resource_id=key.id(), "queued converted replica header");
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
            let rows = self.storage.query("SELECT seq,source_user_id,kind,resource_id,changes_json FROM visible_sync_outbox WHERE hub_id=? AND account_id=? AND seq>? AND seq<=? ORDER BY seq LIMIT 128", &[&binding.scope.hub_id,&binding.scope.account_id,&after.to_string(),&through.to_string()]).await?;
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
                let source = store::text(&row, "source_user_id")?;
                let mut read = self.storage.begin_read_transaction().await?;
                let original = store::binding(&mut *read, &binding.scope, &source, self.peer_id())
                    .await?
                    .ok_or_else(changed)?;
                read.commit().await?;
                let wire = native
                    .into_iter()
                    .map(|change| talos_sync_parser::encode_change(&original, &resource, change))
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

async fn prepared_header_marker(
    tx: &mut dyn talos_storage::StorageTransaction,
    binding: &SyncBinding,
    key: &SyncResourceKey,
) -> TalosResult<bool> {
    let affected = tx.execute("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,created_at)
        SELECT b.hub_id,b.account_id,b.source_user_id,?,?,'[]',? FROM sync_bindings b
        WHERE b.hub_id=? AND b.account_id=? AND NOT EXISTS (
            SELECT 1 FROM visible_sync_outbox o WHERE o.hub_id=b.hub_id AND o.account_id=b.account_id AND o.kind=? AND o.resource_id=?
        ) ORDER BY b.source_user_id LIMIT 1", &[key.kind().as_str(),key.id(),&store::timestamp()?.to_string(),&binding.scope.hub_id,&binding.scope.account_id,key.kind().as_str(),key.id()]).await?;
    Ok(affected > 0)
}
