//! Project committed Store revisions into the existing outbox.
use std::{sync::Arc, time::Duration};

use crate::capture::source::{SessionRevision, SourceCut, SourceFields};
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
use talos_sync::{
    SyncActiveTurnMetadata, SyncCaptureParams, SyncChange, SyncExportFacts, SyncResourceKey,
    TalosSync,
};

use super::{capture, catalog, codec, metadata, reclaim, session, timeline, StoreSyncAdapter};

// Page by resource so an unreplayable revision only holds its own resource.
const RESOURCES_PER_PASS: usize = 32;

impl StoreSyncAdapter {
    /// Start collection for a persisted binding independently of transport.
    /// The task stops when the owning stores release this binding.
    pub fn start_capture(self: &Arc<Self>) {
        let owner = Arc::downgrade(self);
        let wakeup = self.capture_wakeup.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            let mut after = String::new();
            let mut projection_after = String::new();
            let mut ready = false;
            loop {
                if ready {
                    tokio::task::yield_now().await;
                } else {
                    tokio::select! {
                        _ = interval.tick() => {},
                        _ = wakeup.notified() => {},
                    }
                }
                let Some(owner) = owner.upgrade() else { break };
                match owner.collect_page(&after).await {
                    Ok((next, progress)) => {
                        ready = progress || !next.is_empty();
                        after = next;
                    }
                    Err(error) => {
                        ready = false;
                        tracing::warn!(code = ?error.error_code(), "Read pending Store revisions");
                    }
                }
                match owner
                    .sync
                    .backfill_session_projections(&owner.binding, &projection_after)
                    .await
                {
                    Ok(next) => projection_after = next,
                    Err(error) => {
                        tracing::warn!(code = ?error.error_code(), "Read retained Session projections")
                    }
                }
                match reclaim::retired_history(&owner).await {
                    Ok(cleaned) => ready |= cleaned,
                    Err(error) => {
                        tracing::warn!(code = ?error.error_code(), "Reclaim retired Session history")
                    }
                }
                match owner.sync.reclaim_snapshot().await {
                    Ok(cleaned) => ready |= cleaned,
                    Err(error) => {
                        tracing::warn!(code = ?error.error_code(), "Reclaim retired Sync snapshot")
                    }
                }
                match owner.sync.reclaim_capture_batch().await {
                    Ok(cleaned) => ready |= cleaned,
                    Err(error) => {
                        tracing::warn!(code = ?error.error_code(), "Reclaim staged capture batch")
                    }
                }
            }
        });
    }

    async fn collect_page(&self, after: &str) -> TalosResult<(String, bool)> {
        let rows = self
            .storage
            .query(
                "SELECT DISTINCT r.kind, r.resource_id FROM resource_revisions r
             WHERE r.kind <> 'computer' AND NOT EXISTS (SELECT 1 FROM sessions legacy WHERE r.kind='session' AND legacy.session_key=r.resource_id AND legacy.execution_architecture='legacy_read_only') AND r.owner_user_id IN (?2, ?3) AND r.kind || ':' || r.resource_id>?4 AND r.revision>COALESCE((
                 SELECT MAX(c.revision) FROM sync_resource_capture c
                 WHERE c.hub_id=?1 AND c.account_id=?2 AND c.kind=r.kind AND c.resource_id=r.resource_id
             ),0)
             ORDER BY r.kind,r.resource_id LIMIT ?5",
                &[
                    &self.binding.scope.hub_id,
                    &self.binding.scope.account_id,
                    &self.binding.source_user_id,
                    after,
                    &RESOURCES_PER_PASS.to_string(),
                ],
            )
            .await?;
        let mut cursor = String::new();
        let mut progress = false;
        for row in &rows {
            let kind = metadata::text(row, "kind")?;
            let id = metadata::text(row, "resource_id")?;
            cursor = format!("{kind}:{id}");
            let resource = catalog::resource(&kind, id)?;
            match self.collect_revision(&resource).await {
                Ok(committed) => progress |= committed,
                Err(error) => {
                    tracing::warn!(resource = %cursor, code = ?error.error_code(), "Project committed Store revision")
                }
            }
        }
        if rows.len() < RESOURCES_PER_PASS {
            cursor.clear();
        }
        Ok((cursor, progress))
    }

    async fn watermark(
        &self,
        tx: &mut dyn StorageTransaction,
        resource: &SyncResourceKey,
    ) -> TalosResult<i64> {
        let rows = tx.query("SELECT MAX(revision) AS revision FROM sync_resource_capture WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=?", &[&self.binding.scope.hub_id, &self.binding.scope.account_id, resource.kind().as_str(), resource.id()]).await?;
        Ok(rows
            .first()
            .and_then(|row| row.get_i64("revision"))
            .unwrap_or(0))
    }

    async fn next_source(
        &self,
        tx: &mut dyn StorageTransaction,
        resource: &SyncResourceKey,
        after: i64,
    ) -> TalosResult<Option<Row>> {
        Ok(tx.query("SELECT revision, execution_node_id, changes_json FROM resource_revisions WHERE kind=? AND resource_id=? AND owner_user_id IN (?,?) AND revision>? ORDER BY revision LIMIT 1", &[resource.kind().as_str(), resource.id(), &self.binding.source_user_id, &self.binding.scope.account_id, &after.to_string()]).await?.into_iter().next())
    }

    pub(super) async fn has_source_gap(
        &self,
        tx: &mut dyn StorageTransaction,
        resource: &SyncResourceKey,
    ) -> TalosResult<bool> {
        let cut = self.watermark(tx, resource).await?;
        let Some(row) = self.next_source(tx, resource, cut).await? else {
            return Ok(false);
        };
        self.eligible(tx, resource, &row).await
    }

    async fn eligible(
        &self,
        tx: &mut dyn StorageTransaction,
        resource: &SyncResourceKey,
        row: &Row,
    ) -> TalosResult<bool> {
        if let SyncResourceKey::Session { session_id } = resource {
            let existing = tx
                .query(
                    "SELECT execution_architecture FROM sessions WHERE session_key=?",
                    &[session_id],
                )
                .await?;
            if existing.first().is_some_and(|session| {
                session.get_string("execution_architecture").as_deref() == Some("legacy_read_only")
            }) {
                return Ok(false);
            }
            if existing.is_empty() {
                let revision: SessionRevision =
                    serde_json::from_str(&metadata::text(row, "changes_json")?).map_err(codec)?;
                let source = revision.after.as_ref().or(revision.before.as_ref());
                let architecture = source
                    .and_then(|cut| cut.fields.as_ref())
                    .and_then(|fields| fields.row().get_string("execution_architecture"));
                if architecture
                    .as_deref()
                    .is_none_or(|kind| kind == "legacy_read_only")
                {
                    return Ok(false);
                }
            }
        }
        Ok(metadata::text(row, "execution_node_id")? != "embedded"
            || TalosSync::captures_embedded_resource(
                tx,
                &self.binding,
                resource.kind(),
                resource.id(),
            )
            .await?)
    }

    pub(super) async fn collect_revision(&self, resource: &SyncResourceKey) -> TalosResult<bool> {
        let kind = resource.kind().as_str();
        let id = resource.id();
        let mut read = self.storage.begin_read_transaction().await?;
        let previous = self.watermark(&mut *read, resource).await?;
        let Some(row) = self.next_source(&mut *read, resource, previous).await? else {
            return Ok(false);
        };
        if !self.eligible(&mut *read, resource, &row).await? {
            return Ok(false);
        }
        let revision = metadata::number(&row, "revision")?;
        let raw = metadata::text(&row, "changes_json")?;
        let known = capture::has_baseline(self, &mut *read, resource).await?;
        let facts = if let SyncResourceKey::Session { .. } = resource {
            let source: SessionRevision = serde_json::from_str(&raw).map_err(codec)?;
            self.project_revision(&mut *read, id, known, source).await?
        } else {
            capture::project_revision(self, resource, known, &raw)?
        };
        read.commit().await?;
        let prepared = TalosSync::prepare_capture(SyncCaptureParams {
            binding: self.binding.clone(),
            resource: resource.clone(),
            changes: facts.changes,
            created_at: chrono::Utc::now().timestamp(),
        })?;
        let (prepared, mappings) = self
            .sync
            .stage_capture(prepared, facts.event_mappings)
            .await?;

        let mut write = self.storage.begin_background_transaction().await?;
        if self.watermark(&mut *write, resource).await? != previous
            || !self.eligible(&mut *write, resource, &row).await?
            || capture::has_baseline(self, &mut *write, resource).await? != known
        {
            write.rollback().await?;
            return Ok(true);
        }
        self.sync.append_capture(&mut *write, prepared).await?;
        for mapping in mappings {
            self.sync.map_event(&mut *write, mapping).await?;
        }
        write.execute(
            "WITH captured(sequence) AS (SELECT MAX(
                COALESCE((SELECT MAX(seq) FROM visible_sync_outbox WHERE hub_id=?1 AND account_id=?2 AND kind=?3 AND resource_id=?4),0),
                COALESCE((SELECT outbox_sequence FROM sync_resource_capture WHERE hub_id=?1 AND account_id=?2 AND kind=?3 AND resource_id=?4 ORDER BY revision DESC LIMIT 1),0)
             ))
             INSERT INTO sync_resource_capture(hub_id, account_id, kind, resource_id, revision, outbox_sequence, exported_version_json)
             SELECT ?1,?2,?3,?4,?5,sequence,(SELECT exported_version_json FROM visible_sync_outbox WHERE seq=sequence) FROM captured",
            &[&self.binding.scope.hub_id, &self.binding.scope.account_id, kind, id, &revision.to_string()],
        ).await?;
        // Keep the newest source cut for flush and reclaim only revisions
        // already transferred to the outbox for every matching saved binding.
        write
            .execute(
                "DELETE FROM resource_revisions WHERE revision IN (
                SELECT r.revision FROM resource_revisions r
                WHERE r.kind=?1 AND r.resource_id=?2 AND r.revision<?3 AND r.owner_user_id IN (?4,?5)
                  AND NOT EXISTS (
                    SELECT 1 FROM sync_bindings b
                    WHERE r.owner_user_id IN (b.account_id,b.source_user_id)
                      AND NOT EXISTS (
                        SELECT 1 FROM sync_resource_capture c
                        WHERE c.hub_id=b.hub_id AND c.account_id=b.account_id
                          AND c.kind=r.kind AND c.resource_id=r.resource_id AND c.revision=r.revision
                    )
                  )
                ORDER BY r.revision LIMIT 32
             )",
                &[
                    kind, id,
                    &revision.to_string(),
                    &self.binding.scope.account_id,
                    &self.binding.source_user_id,
                ],
            )
            .await?;
        write.commit().await?;
        self.committed();
        Ok(true)
    }

    pub(super) async fn project_revision(
        &self,
        tx: &mut dyn StorageTransaction,
        id: &str,
        known: bool,
        source: SessionRevision,
    ) -> TalosResult<SyncExportFacts> {
        let mut facts = SyncExportFacts::default();
        let before = source.before.as_ref();
        if !known {
            if let Some(before) = before {
                for history_generation in 1..=before.generation {
                    facts.changes.push(SyncChange::Reset { history_generation });
                }
                facts.changes.push(SyncChange::Metadata {
                    before: None,
                    after: Box::new(
                        metadata::project_session(
                            self,
                            tx,
                            id,
                            before.generation,
                            &source_fields(before)?,
                        )
                        .await?,
                    ),
                });
                self.project_execution(tx, id, None, before, &mut facts)
                    .await?;
            }
        }
        let Some(after) = source.after.as_ref() else {
            facts.changes.push(SyncChange::Delete);
            return Ok(facts);
        };
        for history_generation in before.map_or(0, |cut| cut.generation) + 1..=after.generation {
            facts.changes.push(SyncChange::Reset { history_generation });
        }
        if before.is_none_or(|before| before.fields != after.fields) {
            let old = match before {
                Some(old) => Some(Box::new(
                    metadata::project_session(self, tx, id, old.generation, &source_fields(old)?)
                        .await?,
                )),
                None => None,
            };
            facts.changes.push(SyncChange::Metadata {
                before: old,
                after: Box::new(
                    metadata::project_session(
                        self,
                        tx,
                        id,
                        after.generation,
                        &source_fields(after)?,
                    )
                    .await?,
                ),
            });
        }
        self.project_execution(tx, id, before, after, &mut facts)
            .await?;
        Ok(facts)
    }

    async fn project_execution(
        &self,
        tx: &mut dyn StorageTransaction,
        id: &str,
        previous: Option<&SourceCut>,
        source: &SourceCut,
        facts: &mut SyncExportFacts,
    ) -> TalosResult<()> {
        let previous = previous.filter(|old| old.generation == source.generation);
        let event_tail = previous.map_or(0, |old| old.event_tail);
        let command_tail = previous.map_or(0, |old| old.command_tail);
        if source.event_tail > event_tail {
            let events = timeline::read_events(
                self,
                tx,
                id,
                source.generation,
                event_tail,
                source.event_tail,
            )
            .await?;
            facts.changes.extend(events.changes);
            facts.event_mappings.extend(events.event_mappings);
        }
        if let Some(active) = source
            .active
            .as_ref()
            .filter(|active| previous.and_then(|old| old.active.as_ref()) != Some(*active))
        {
            if let Some(last_event) =
                session::key_at_generation(self, tx, id, source.generation, active.event_seq)
                    .await?
            {
                facts
                    .changes
                    .push(SyncChange::ActiveTurnMetadata(SyncActiveTurnMetadata {
                        identity: talos_types::AgentTurnRef::new(id, &active.turn_id),
                        state: active.state,
                        updated_at: active.updated_at,
                        last_event,
                    }));
            }
        }
        if source.command_tail > command_tail {
            let rows = tx.query("SELECT * FROM session_command_revisions WHERE session_id=? AND history_generation=? AND revision>? AND revision<=? ORDER BY revision", &[id, &source.generation.to_string(), &command_tail.to_string(), &source.command_tail.to_string()]).await?;
            facts
                .changes
                .extend(timeline::project_commands(self, tx, id, source.generation, rows).await?);
        }
        Ok(())
    }
}

fn source_fields(cut: &SourceCut) -> TalosResult<Row> {
    cut.fields
        .as_ref()
        .map(SourceFields::row)
        .ok_or_else(|| TalosError::internal("Session revision is missing its metadata baseline."))
}
