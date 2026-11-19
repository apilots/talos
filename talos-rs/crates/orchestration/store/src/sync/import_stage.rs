use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
use talos_sync::{same_version, SyncImportBatch, SyncResourceKey, SyncVersion};
use talos_sync_types::SyncChange;

use super::{codec, metadata, session, StoreSyncAdapter};

const MAX_EVENTS_PER_WRITE: usize = 128;
const MAX_EVENT_BYTES_PER_WRITE: usize = 256 * 1024;

#[derive(PartialEq, Eq)]
struct SessionCut {
    generation: i64,
    event_seq: i64,
}

struct ImportStage {
    id: i64,
    base: SessionCut,
    version: SyncVersion,
    next_event: usize,
    complete: bool,
}

async fn session_cut(tx: &mut dyn StorageTransaction, id: &str) -> TalosResult<SessionCut> {
    let rows = tx.query(
        "SELECT COALESCE((SELECT history_generation FROM sessions WHERE session_key=?1),-1) AS generation,
                COALESCE((SELECT MAX(event_seq) FROM visible_agent_events WHERE session_id=?1),0) AS event_seq",
        &[id],
    ).await?;
    Ok(SessionCut {
        generation: metadata::number(&rows[0], "generation")?,
        event_seq: metadata::number(&rows[0], "event_seq")?,
    })
}

async fn pending(
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
) -> TalosResult<Option<ImportStage>> {
    let rows = tx.query(
        "SELECT id, base_generation, base_event_seq, version_json, next_event, complete FROM sync_import_stages
         WHERE hub_id=? AND account_id=? AND session_id=? AND published=0",
        &[&batch.scope.hub_id, &batch.scope.account_id, batch.resource.id()],
    ).await?;
    rows.first().map(parse).transpose()
}

fn parse(row: &Row) -> TalosResult<ImportStage> {
    Ok(ImportStage {
        id: metadata::number(row, "id")?,
        base: SessionCut {
            generation: metadata::number(row, "base_generation")?,
            event_seq: metadata::number(row, "base_event_seq")?,
        },
        version: serde_json::from_str(&metadata::text(row, "version_json")?).map_err(codec)?,
        next_event: usize::try_from(metadata::number(row, "next_event")?)
            .map_err(|_| TalosError::internal("Invalid synchronization staging cursor."))?,
        complete: metadata::number(row, "complete")? == 1,
    })
}

pub(super) async fn prepare(
    adapter: &StoreSyncAdapter,
    batch: &SyncImportBatch,
) -> TalosResult<()> {
    let deletes_session = matches!(batch.resource, SyncResourceKey::Session { .. })
        && batch
            .changes
            .iter()
            .any(|change| matches!(change, SyncChange::Delete));
    if !deletes_session && !needs_staging(batch) {
        return Ok(());
    }
    let mut read = adapter.storage.begin_read_transaction().await?;
    let existing_stage = pending(&mut *read, batch).await?;
    if deletes_session {
        read.commit().await?;
        if let Some(stage) = existing_stage {
            discard(adapter, stage.id).await?;
        }
        return Ok(());
    }
    let base = session_cut(&mut *read, batch.resource.id()).await?;
    let resumable = match &existing_stage {
        Some(stage) => stage.base == base && same_version(&stage.version, &batch.version)?,
        None => false,
    };
    if resumable && existing_stage.as_ref().is_some_and(|stage| stage.complete) {
        read.commit().await?;
        return Ok(());
    }
    let entries = session::ordered_events(&batch.changes)?;
    let mut events = Vec::new();
    for page in entries.chunks(MAX_EVENTS_PER_WRITE) {
        events.extend(session::new_events(&mut *read, batch, page).await?);
    }
    read.commit().await?;

    let (stage_id, start) = if let Some(stage) = existing_stage {
        if resumable {
            (stage.id, stage.next_event)
        } else {
            discard(adapter, stage.id).await?;
            (create(adapter, batch, &base).await?, 0)
        }
    } else {
        (create(adapter, batch, &base).await?, 0)
    };

    let mut offset = start;
    loop {
        let remaining = events.get(offset..).ok_or_else(stale)?;
        let mut bytes = 0;
        let mut next = offset;
        for event in remaining.iter().take(MAX_EVENTS_PER_WRITE) {
            bytes += event.bytes();
            if next > offset && bytes > MAX_EVENT_BYTES_PER_WRITE {
                break;
            }
            next += 1;
        }
        let done = next == events.len();
        let mut write = adapter.storage.begin_background_transaction().await?;
        let advanced = write.execute(
            "UPDATE sync_import_stages SET next_event=?, complete=? WHERE id=? AND published=0 AND next_event=?",
            &[&next.to_string(), if done { "1" } else { "0" }, &stage_id.to_string(), &offset.to_string()],
        ).await?;
        if advanced != 1 {
            return Err(stale());
        }
        for event in &events[offset..next] {
            event.insert(&mut *write, batch, stage_id).await?;
        }
        write.commit().await?;
        if done {
            return Ok(());
        }
        offset = next;
        tokio::task::yield_now().await;
    }
}

fn needs_staging(batch: &SyncImportBatch) -> bool {
    matches!(batch.resource, SyncResourceKey::Session { .. })
        && batch
            .changes
            .iter()
            .any(|change| matches!(change, SyncChange::Event(_)))
        && !batch
            .changes
            .iter()
            .any(|change| matches!(change, SyncChange::Delete))
}

async fn create(
    adapter: &StoreSyncAdapter,
    batch: &SyncImportBatch,
    base: &SessionCut,
) -> TalosResult<i64> {
    let encoded = serde_json::to_string(&batch.version).map_err(codec)?;
    let mut tx = adapter.storage.begin_background_transaction().await?;
    let rows = tx.query(
        "INSERT INTO sync_import_stages(hub_id, account_id, session_id, version_json, base_generation, base_event_seq)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        &[&batch.scope.hub_id, &batch.scope.account_id, batch.resource.id(), &encoded, &base.generation.to_string(), &base.event_seq.to_string()],
    ).await?;
    let id = metadata::number(&rows[0], "id")?;
    tx.commit().await?;
    Ok(id)
}

async fn discard(adapter: &StoreSyncAdapter, id: i64) -> TalosResult<()> {
    let id = id.to_string();
    let limit = MAX_EVENTS_PER_WRITE.to_string();
    loop {
        let mut tx = adapter.storage.begin_background_transaction().await?;
        if tx
            .query(
                "SELECT 1 FROM sync_import_stages WHERE id=? AND published=0",
                &[&id],
            )
            .await?
            .is_empty()
        {
            return Err(stale());
        }
        tx.execute(
            "DELETE FROM sync_event_map WHERE import_id=?1 AND local_event_seq IN (
                SELECT event_seq FROM agent_events WHERE import_id=?1 ORDER BY event_seq LIMIT ?2
            )",
            &[&id, &limit],
        )
        .await?;
        let removed = tx
            .execute(
                "DELETE FROM agent_events WHERE event_seq IN (
                SELECT event_seq FROM agent_events WHERE import_id=?1 ORDER BY event_seq LIMIT ?2
            )",
                &[&id, &limit],
            )
            .await?;
        if removed < MAX_EVENTS_PER_WRITE as u64 {
            tx.execute(
                "DELETE FROM sync_import_stages WHERE id=? AND published=0",
                &[&id],
            )
            .await?;
            tx.commit().await?;
            return Ok(());
        }
        tx.commit().await?;
        tokio::task::yield_now().await;
    }
}

pub(super) async fn publish(
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
) -> TalosResult<bool> {
    if !needs_staging(batch) {
        return Ok(false);
    }
    let stage = pending(tx, batch).await?.ok_or_else(stale)?;
    if !stage.complete
        || !same_version(&stage.version, &batch.version)?
        || stage.base != session_cut(tx, batch.resource.id()).await?
    {
        return Err(stale());
    }
    let publish = if stage.next_event == 0 {
        "DELETE FROM sync_import_stages WHERE id=?"
    } else {
        "UPDATE sync_import_stages SET published=1 WHERE id=?"
    };
    tx.execute(publish, &[&stage.id.to_string()]).await?;
    Ok(true)
}

fn stale() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The staged Session import changed before publication.",
    )
}
