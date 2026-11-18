use std::sync::Arc;

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use talos_sync_sdk::{version_equal, version_includes, version_union, SyncReplica};
use talos_sync_types::SyncResourceSummary;

use super::{sync_error, TalosSync};
use crate::{projection, store, SyncBinding, SyncExportBatch, SyncResourceKey, SyncVersion};

const MAX_CONCURRENT_RETRIES: usize = 3;
const OUTBOX_PAGE_SIZE: usize = 128;

#[derive(Clone, Copy)]
enum OutboxCut {
    Flush,
    Export(usize),
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
}

struct ReplicaWrite {
    summary: SyncResourceSummary,
    snapshot: Vec<u8>,
    acknowledged: SyncVersion,
    exported: Vec<(i64, String)>,
}

impl TalosSync {
    /// Fix the flush boundary to the outbox rows visible at the original read cut.
    pub(crate) async fn flush_cut(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
    ) -> TalosResult<(Option<i64>, SyncVersion)> {
        let result = self
            .materialize(binding, resource, OutboxCut::Flush, |_, _| Ok(()))
            .await?
            .expect("flush prepares a version even without pending changes");
        if let Some(tx) = result.transaction {
            tx.commit().await?;
        }
        Ok((result.through, result.version))
    }

    /// Form and persist one fixed upload cut. A lost ACK retains the same facts.
    pub async fn export_pending(
        &self,
        binding: &SyncBinding,
        resource: SyncResourceKey,
        limit: usize,
    ) -> TalosResult<Option<SyncExportBatch>> {
        if !(1..=1024).contains(&limit) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid synchronization batch limit.",
            ));
        }
        let resource = projection::resource(binding, &resource);
        let Some(result) = self
            .materialize(
                binding,
                &resource,
                OutboxCut::Export(limit),
                |replica, acknowledged| replica.export_since(acknowledged).map_err(sync_error),
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

    /// Prepare downloaded facts without the writer, then stage the merge and
    /// return its transaction. The host applies the original Store facts in
    /// that transaction before committing and marking the version as committed.
    pub async fn import_document(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        updates: &[Vec<u8>],
        downloaded_version: &SyncVersion,
        download_cursor: Option<&str>,
    ) -> TalosResult<(
        Box<dyn StorageTransaction>,
        SyncVersion,
        Vec<talos_sync_types::SyncChange>,
    )> {
        let updates = Arc::<[Vec<u8>]>::from(updates);
        let downloaded_version = downloaded_version.clone();
        let writer_id = self.writer_id;
        let result = self
            .materialize(
                binding,
                resource,
                OutboxCut::Import,
                move |replica, acknowledged| {
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
                    Ok((
                        replica
                            .deleted_at(&downloaded_version)
                            .map_err(sync_error)?,
                        replica.changes().map_err(sync_error)?,
                    ))
                },
            )
            .await?
            .expect("import prepares a version even without pending changes");
        let (deleted_on_hub, changes) = result.value;
        let mut tx = result.transaction.expect("document import stages a write");
        if deleted_on_hub {
            // A committed hub tombstone supersedes local facts, including a
            // local pending delete that has never been acknowledged.
            tx.execute("DELETE FROM sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id()]).await?;
        }
        if let Some(cursor) = download_cursor {
            tx.execute(
                "UPDATE sync_bindings SET download_cursor = ? WHERE hub_id = ? AND account_id = ?",
                &[cursor, &binding.scope.hub_id, &binding.scope.account_id],
            )
            .await?;
        }
        Ok((tx, result.version, changes))
    }

    async fn materialize<T: Send + 'static>(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        cut: OutboxCut,
        apply: impl Fn(&mut SyncReplica, &mut SyncVersion) -> TalosResult<T> + Send + Sync + 'static,
    ) -> TalosResult<Option<Materialized<T>>> {
        let importing = matches!(cut, OutboxCut::Import);
        let apply = Arc::new(apply);
        let mut through = i64::MAX;
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
            let writer_id = self.writer_id;
            let (mut version, value, write) = tokio::task::spawn_blocking(move || {
                let (mut replica, mut acknowledged) =
                    Self::restore_replica(writer_id, &resource_key, input.record)?;
                let mut exported = Vec::new();
                for row in input.rows {
                    if row.exported_version.is_none() {
                        replica.apply(&row.changes).map_err(sync_error)?;
                        // Freeze each local prefix before merging any remote operations.
                        let version = store::encode(&replica.version().map_err(sync_error)?)?;
                        exported.push((row.seq, version));
                    }
                }
                let value = apply(&mut replica, &mut acknowledged)?;
                let version = replica.version().map_err(sync_error)?;
                let write = if importing || !exported.is_empty() {
                    Some(ReplicaWrite {
                        summary: replica.summary().map_err(sync_error)?,
                        snapshot: replica.snapshot().map_err(sync_error)?,
                        acknowledged,
                        exported,
                    })
                } else {
                    None
                };
                Ok::<_, TalosError>((version, value, write))
            })
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Prepare synchronization replica.", error)
            })??;

            if matches!(cut, OutboxCut::Flush) {
                if let Some(last_exported) = last_exported {
                    version = last_exported;
                }
                if let Some((original_through, original_version)) = &original_flush {
                    if last != Some(*original_through) {
                        // The runner verifies the original minimum against the
                        // hub whether an ACK or a tombstone removed these rows.
                        return Ok(Some(Materialized {
                            transaction: None,
                            through: Some(*original_through),
                            version: original_version.clone(),
                            value,
                        }));
                    }
                } else if let Some(last) = last {
                    original_flush = Some((last, version.clone()));
                }
            }

            let transaction = if let Some(write) = write {
                let mut tx = self.storage.begin_transaction().await?;
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
                    &write.snapshot,
                    &write.acknowledged,
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
            OutboxCut::Export(limit) => limit,
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
            let last_page = page.len() < limit || matches!(cut, OutboxCut::Export(_));
            rows.extend(page);
            if last_page {
                break;
            }
        }
        if matches!(cut, OutboxCut::Export(_)) && rows.is_empty() {
            tx.commit().await?;
            return Ok(None);
        }
        let record = store::document(&mut *tx, &binding.scope, resource).await?;
        tx.commit().await?;
        Ok(Some(ReplicaInput { record, rows }))
    }
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
