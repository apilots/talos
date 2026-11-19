use talos_error::TalosResult;

use super::{metadata, StoreSyncAdapter};

const ROW_BUDGET: u64 = 128;

const READY_HISTORY: &str = "
    SELECT r.session_id, r.through_generation FROM session_event_retention r
    WHERE r.reclaim=1 AND NOT EXISTS (
        SELECT 1 FROM resource_revisions v
        JOIN sync_bindings b ON v.owner_user_id IN (b.source_user_id,b.account_id)
        WHERE v.kind='session' AND v.resource_id=r.session_id
          AND NOT EXISTS (
              SELECT 1 FROM sync_resource_capture c
              WHERE c.hub_id=b.hub_id AND c.account_id=b.account_id
                AND c.kind=v.kind AND c.resource_id=v.resource_id AND c.revision=v.revision
          )
    )
    ORDER BY r.session_id LIMIT 1";

pub(super) async fn retired_history(adapter: &StoreSyncAdapter) -> TalosResult<bool> {
    if adapter.storage.query(READY_HISTORY, &[]).await?.is_empty() {
        return Ok(false);
    }
    let mut tx = adapter.storage.begin_background_transaction().await?;
    let ready = tx.query(READY_HISTORY, &[]).await?;
    let Some(row) = ready.first() else {
        tx.commit().await?;
        return Ok(false);
    };
    let id = metadata::text(row, "session_id")?;
    let generation = metadata::number(row, "through_generation")?.to_string();
    let params: &[&str] = &[&id, &generation, &ROW_BUDGET.to_string()];
    tx.execute(
        "DELETE FROM sync_event_map WHERE local_event_seq IN (
            SELECT event_seq FROM committed_agent_events WHERE session_id=?1 AND history_generation<=?2
            ORDER BY history_generation,event_seq LIMIT ?3
        )",
        params,
    ).await?;
    let events = tx.execute(
        "DELETE FROM agent_events WHERE event_seq IN (
            SELECT event_seq FROM committed_agent_events WHERE session_id=?1 AND history_generation<=?2
            ORDER BY history_generation,event_seq LIMIT ?3
        )",
        params,
    ).await?;
    let remaining = ROW_BUDGET - events;
    if remaining > 0 {
        let commands = tx.execute(
            "DELETE FROM session_command_revisions WHERE revision IN (
                SELECT revision FROM session_command_revisions WHERE session_id=?1 AND history_generation<=?2
                ORDER BY history_generation,revision LIMIT ?3
            )",
            &[&id, &generation, &remaining.to_string()],
        ).await?;
        if commands < remaining {
            let limit = remaining - commands;
            let markers = tx
                .execute(
                    "DELETE FROM sync_import_stages WHERE id IN (
                    SELECT s.id FROM sync_import_stages s WHERE s.session_id=?1 AND s.published=1
                        AND NOT EXISTS (SELECT 1 FROM agent_events e WHERE e.import_id=s.id)
                    ORDER BY s.id LIMIT ?2
                )",
                    &[&id, &limit.to_string()],
                )
                .await?;
            if markers < limit {
                tx.execute(
                    "UPDATE session_event_retention SET reclaim=0 WHERE session_id=?",
                    &[&id],
                )
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(true)
}
