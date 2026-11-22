use std::{future, sync::Arc};

use crate::SyncResourceSummary;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use talos_sync_parser::SessionProjection;
use talos_sync_sdk::{version_equal, version_includes, version_union, SyncReplica};

use super::{sync_error, TalosSync};
use crate::{
    store, SyncBinding, SyncChange, SyncDataAdapter, SyncExportBatch, SyncImportBatch,
    SyncResourceKey, SyncVersion,
};

const MAX_CONCURRENT_RETRIES: usize = 3;
const OUTBOX_PAGE_SIZE: usize = 128;

#[derive(Clone, Copy)]
enum OutboxCut {
    Flush,
    Export { limit: usize, through: i64 },
    Import,
}

struct ReplicaInput {
    record: Option<store::DocumentRecord>,
    rows: Vec<store::OutboxRecord>,
}

struct Materialized<T> {
    transaction: Option<Box<dyn StorageTransaction>>,
    through: Option<i64>,
    version: SyncVersion,
    value: T,
    imported: Vec<SyncChange>,
}

struct ReplicaWrite {
    summary: SyncResourceSummary,
    snapshot: Vec<u8>,
    acknowledged: SyncVersion,
    exported: Vec<(i64, String)>,
    session_projection: Option<String>,
}

impl TalosSync {
    /// Fix the flush boundary to the outbox rows visible at the original read cut.
    pub(crate) async fn flush_cut(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
    ) -> TalosResult<(Option<i64>, SyncVersion)> {
        let result = self
            .materialize(
                binding,
                resource,
                OutboxCut::Flush,
                |_, _, _| Ok(()),
                unchanged_import,
            )
            .await?
            .expect("flush prepares a version even without pending changes");
        if let Some(tx) = result.transaction {
            tx.commit().await?;
        }
        Ok((result.through, result.version))
    }

    pub(crate) async fn captured_version(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        sequence: i64,
    ) -> TalosResult<SyncVersion> {
        // Materialization saves the proof before either Hub ACK or deletion
        // removes the corresponding outbox rows.
        let rows = self.storage.query(
            "SELECT exported_version_json FROM sync_resource_capture WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=? AND outbox_sequence=? AND exported_version_json IS NOT NULL LIMIT 1",
            &[&binding.scope.hub_id,&binding.scope.account_id,resource.kind().as_str(),resource.id(),&sequence.to_string()],
        ).await?;
        let row = rows.first().ok_or_else(|| {
            TalosError::new(
                ErrorCode::Conflict,
                "The captured resource prefix is unavailable.",
            )
        })?;
        store::json(row, "exported_version_json")
    }

    /// Form and persist one fixed upload cut. A lost ACK retains the same facts.
    pub async fn export_pending(
        &self,
        binding: &SyncBinding,
        resource: SyncResourceKey,
        limit: usize,
    ) -> TalosResult<Option<SyncExportBatch>> {
        self.export_pending_through(binding, resource, limit, i64::MAX)
            .await
    }

    pub(crate) async fn export_pending_through(
        &self,
        binding: &SyncBinding,
        resource: SyncResourceKey,
        limit: usize,
        through: i64,
    ) -> TalosResult<Option<SyncExportBatch>> {
        if !(1..=1024).contains(&limit) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid synchronization batch limit.",
            ));
        }
        let resource = talos_sync_parser::resource(binding, &resource);
        let Some(result) = self
            .materialize(
                binding,
                &resource,
                OutboxCut::Export { limit, through },
                |replica, acknowledged, prefix| {
                    replica
                        .export_through(acknowledged, prefix)
                        .map_err(sync_error)
                },
                unchanged_import,
            )
            .await?
        else {
            return Ok(None);
        };
        if let Some(tx) = result.transaction {
            tx.commit().await?;
        }
        Ok(result.through.map(|through_sequence| SyncExportBatch {
            scope: binding.scope.clone(),
            resource,
            through_sequence,
            update: result.value,
            version: result.version,
        }))
    }

    /// Decode the document and prepare Store facts before taking the publication
    /// transaction. The caller applies the returned batch and commits that transaction.
    pub async fn prepare_import(
        &self,
        adapter: &impl SyncDataAdapter,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        updates: &[Vec<u8>],
        downloaded_version: &SyncVersion,
    ) -> TalosResult<(Box<dyn StorageTransaction>, SyncImportBatch)> {
        let updates = Arc::<[Vec<u8>]>::from(updates);
        let downloaded_version = downloaded_version.clone();
        let writer_id = self.writer_id;
        let result = self
            .materialize(
                binding,
                resource,
                OutboxCut::Import,
                move |replica, acknowledged, _| {
                    replica.import(&updates).map_err(sync_error)?;
                    replica.set_writer_id(writer_id).map_err(sync_error)?;
                    if !version_includes(
                        &replica.version().map_err(sync_error)?,
                        &downloaded_version,
                    )
                    .map_err(sync_error)?
                    {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Downloaded synchronization dependencies are incomplete.",
                        ));
                    }
                    *acknowledged =
                        version_union(acknowledged, &downloaded_version).map_err(sync_error)?;
                    replica.deleted_at(&downloaded_version).map_err(sync_error)
                },
                |version, changes| async move {
                    let batch = SyncImportBatch {
                        scope: binding.scope.clone(),
                        resource: resource.clone(),
                        version,
                        changes,
                    };
                    adapter.prepare_import(&batch).await?;
                    Ok(batch.changes)
                },
            )
            .await?
            .expect("import prepares a version even without pending changes");
        let mut tx = result.transaction.expect("document import stages a write");
        if result.value {
            // A committed hub tombstone supersedes local facts, including a
            // local pending delete that has never been acknowledged.
            tx.execute("DELETE FROM sync_outbox WHERE seq IN (SELECT seq FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?)", &[&binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id()]).await?;
        }
        Ok((
            tx,
            SyncImportBatch {
                scope: binding.scope.clone(),
                resource: resource.clone(),
                version: result.version,
                changes: result.imported,
            },
        ))
    }

    async fn materialize<T: Send + 'static, F>(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        cut: OutboxCut,
        apply: impl Fn(&mut SyncReplica, &mut SyncVersion, &SyncVersion) -> TalosResult<T>
            + Send
            + Sync
            + 'static,
        mut prepare: impl FnMut(SyncVersion, Vec<SyncChange>) -> F + Send,
    ) -> TalosResult<Option<Materialized<T>>>
    where
        F: future::Future<Output = TalosResult<Vec<SyncChange>>> + Send,
    {
        let importing = matches!(cut, OutboxCut::Import);
        let apply = Arc::new(apply);
        let mut through = match cut {
            OutboxCut::Export { through, .. } => through,
            _ => i64::MAX,
        };
        let mut original_flush = None::<(i64, SyncVersion)>;
        for _ in 0..MAX_CONCURRENT_RETRIES {
            let Some(input) = self
                .read_replica_input(binding, resource, cut, through)
                .await?
            else {
                return Ok(None);
            };
            let last = input.rows.last().map(|row| row.seq);
            let last_exported = input
                .rows
                .last()
                .and_then(|row| row.exported_version.clone());
            if !importing {
                through = last.unwrap_or(0);
            }
            let base = input
                .record
                .as_ref()
                .map(|record| (record.version.clone(), record.acknowledged.clone()));
            let apply = apply.clone();
            let resource_key = resource.clone();
            let account = binding.scope.account_id.clone();
            let writer_id = self.writer_id;
            let (version, value, write, imported) = tokio::task::spawn_blocking(move || {
                let mut refresh_session_projection = importing
                    || input
                        .record
                        .as_ref()
                        .is_none_or(|record| !record.has_session_projection);
                let (mut replica, mut acknowledged) =
                    Self::restore_replica(writer_id, &resource_key, input.record)?;
                let pending = input
                    .rows
                    .iter()
                    .filter(|row| row.exported_version.is_none());
                if !refresh_session_projection {
                    'batches: for row in pending.clone() {
                        for change in &row.changes {
                            if SessionProjection::needs_refresh(change)? {
                                refresh_session_projection = true;
                                break 'batches;
                            }
                        }
                    }
                }
                // Freeze each local prefix before merging any remote operations.
                let versions = replica
                    .apply_batches(pending.clone().map(|row| row.changes.as_slice()))
                    .map_err(sync_error)?;
                let exported = pending
                    .zip(versions)
                    .map(|(row, version)| Ok((row.seq, store::encode(&version)?)))
                    .collect::<TalosResult<Vec<_>>>()?;
                let prefix =
                    last_exported.map_or_else(|| replica.version().map_err(sync_error), Ok)?;
                let value = apply(&mut replica, &mut acknowledged, &prefix)?;
                let version = if importing {
                    replica.version().map_err(sync_error)?
                } else {
                    prefix
                };
                let imported = if importing {
                    crate::facade::replica_changes(&replica, &resource_key, &account)?
                } else {
                    Vec::new()
                };
                let write = if importing || !exported.is_empty() {
                    let snapshot = if refresh_session_projection
                        && matches!(resource_key, SyncResourceKey::Session { .. })
                    {
                        let encoded = if importing {
                            store::encode(&SessionProjection::from_changes(&imported))?
                        } else {
                            store::encode(&SessionProjection::from_changes(
                                &crate::facade::replica_changes(&replica, &resource_key, &account)?,
                            ))?
                        };
                        Some(encoded)
                    } else {
                        None
                    };
                    Some(ReplicaWrite {
                        summary: replica.summary().map_err(sync_error)?.try_into()?,
                        snapshot: replica.snapshot().map_err(sync_error)?,
                        acknowledged,
                        exported,
                        session_projection: snapshot,
                    })
                } else {
                    None
                };
                Ok::<_, TalosError>((version, value, write, imported))
            })
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Prepare synchronization replica.", error)
            })??;

            if matches!(cut, OutboxCut::Flush) {
                if let Some((original_through, original_version)) = &original_flush {
                    if last != Some(*original_through) {
                        // The runner verifies the original minimum against the
                        // hub whether an ACK or a tombstone removed these rows.
                        return Ok(Some(Materialized {
                            transaction: None,
                            through: Some(*original_through),
                            version: original_version.clone(),
                            value,
                            imported,
                        }));
                    }
                } else if let Some(last) = last {
                    original_flush = Some((last, version.clone()));
                }
            }

            let imported = if importing {
                prepare(version.clone(), imported).await?
            } else {
                imported
            };
            let transaction = if let Some(write) = write {
                let snapshot =
                    store::snapshot::Snapshot::prepare(self.storage.clone(), write.snapshot)
                        .await?;
                let mut tx = self.storage.begin_background_transaction().await?;
                self.require_binding(&mut *tx, binding).await?;
                let current =
                    store::document_checkpoint(&mut *tx, &binding.scope, resource).await?;
                if !checkpoint_matches(current.as_ref(), base.as_ref())?
                    || (importing
                        && store::outbox_tail(&mut *tx, &binding.scope, resource).await? != last)
                {
                    tx.rollback().await?;
                    continue;
                }
                store::save_document(
                    &mut *tx,
                    &binding.scope,
                    &write.summary,
                    &snapshot,
                    &write.acknowledged,
                    write.session_projection.as_deref(),
                )
                .await?;
                for (seq, version) in write.exported {
                    let updated = tx.execute(
                        "UPDATE sync_outbox SET exported_version_json = ? WHERE seq = ? AND exported_version_json IS NULL",
                        &[&version, &seq.to_string()],
                    ).await?;
                    if updated != 1 {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Synchronization outbox changed during replica preparation.",
                        ));
                    }
                    tx.execute(
                        "UPDATE sync_resource_capture SET exported_version_json=? WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=? AND outbox_sequence=?",
                        &[&version, &binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id(), &seq.to_string()],
                    ).await?;
                }
                Some(tx)
            } else {
                None
            };
            return Ok(Some(Materialized {
                transaction,
                through: last,
                version,
                value,
                imported,
            }));
        }
        Err(TalosError::new(
            ErrorCode::Conflict,
            "Synchronization document changed during replica preparation.",
        ))
    }

    async fn read_replica_input(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        cut: OutboxCut,
        through: i64,
    ) -> TalosResult<Option<ReplicaInput>> {
        let mut tx = self.storage.begin_read_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let limit = match cut {
            OutboxCut::Export { limit, .. } => limit,
            OutboxCut::Flush | OutboxCut::Import => OUTBOX_PAGE_SIZE,
        };
        let mut rows = Vec::new();
        let mut after = 0;
        loop {
            let page =
                store::outbox_through(&mut *tx, &binding.scope, resource, after, through, limit)
                    .await?;
            let Some(last) = page.last() else {
                break;
            };
            after = last.seq;
            let last_page = page.len() < limit || matches!(cut, OutboxCut::Export { .. });
            rows.extend(page);
            if last_page {
                break;
            }
        }
        if matches!(cut, OutboxCut::Export { .. }) && rows.is_empty() {
            tx.commit().await?;
            return Ok(None);
        }
        let record = store::document(&mut *tx, &binding.scope, resource).await?;
        tx.commit().await?;
        Ok(Some(ReplicaInput { record, rows }))
    }
}

fn unchanged_import(
    _: SyncVersion,
    changes: Vec<SyncChange>,
) -> impl future::Future<Output = TalosResult<Vec<SyncChange>>> + Send {
    future::ready(Ok(changes))
}

fn checkpoint_matches(
    current: Option<&(SyncVersion, SyncVersion)>,
    expected: Option<&(SyncVersion, SyncVersion)>,
) -> TalosResult<bool> {
    match (current, expected) {
        (Some((current, acknowledged)), Some((expected, expected_acknowledged))) => {
            Ok(version_equal(current, expected).map_err(sync_error)?
                && version_equal(acknowledged, expected_acknowledged).map_err(sync_error)?)
        }
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}
