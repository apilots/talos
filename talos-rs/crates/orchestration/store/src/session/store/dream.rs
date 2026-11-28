//! Product consumption and eligibility for retained Worker Dream execution facts.

use super::DatabaseSessionStore;
use talos_error::{TalosError, TalosResult};

pub(super) async fn read_cursor(
    store: &DatabaseSessionStore,
    session_id: &str,
) -> TalosResult<Option<agent_protocol::DreamMemoryCursor>> {
    let rows = store
        .storage
        .query(
            "SELECT generation,formation_offset FROM session_dream_cursors WHERE session_id=?",
            &[session_id],
        )
        .await?;
    rows.first()
        .map(|row| {
            Ok(agent_protocol::DreamMemoryCursor {
                generation: u64::try_from(row.get_i64("generation").unwrap_or(-1)).map_err(
                    |error| {
                        TalosError::internal_with_source(
                            "Read Dream Memory consumption generation.",
                            error,
                        )
                    },
                )?,
                offset: row.get_i64("formation_offset").ok_or_else(|| {
                    TalosError::internal("Dream Memory consumption offset is missing.")
                })?,
            })
        })
        .transpose()
}

pub(super) async fn has_pending(
    store: &DatabaseSessionStore,
    session_id: &str,
) -> TalosResult<bool> {
    // Retained source mappings qualify known work. Accepted commands and an
    // incomplete product view remain conservative candidates until source replay.
    let rows = store.storage.query(
        "SELECT 1 AS pending FROM sessions session LEFT JOIN worker_projection_cursors source ON source.session_id=session.session_key LEFT JOIN session_dream_cursors dream ON dream.session_id=session.session_key AND dream.generation=source.history_generation WHERE session.session_key=? AND (EXISTS (SELECT 1 FROM worker_event_projection event WHERE event.session_id=session.session_key AND event.history_generation=source.history_generation AND event.worker_offset>COALESCE(dream.covered_until_source_seq,0) AND json_extract(event.execution_purpose,'$')='turn' AND json_extract(event.settlement_json,'$.type')='finished') OR EXISTS (SELECT 1 FROM session_command_revisions command WHERE command.session_id=session.session_key AND command.history_generation=session.history_generation AND command.outcome='accepted' AND command.committed_event_seq>COALESCE((SELECT MAX(event.product_event_seq) FROM worker_event_projection event WHERE event.session_id=session.session_key AND event.history_generation=source.history_generation AND event.worker_offset<=COALESCE(dream.covered_until_source_seq,0)),0)) OR (source.session_id IS NULL AND (session.latest_turn_seq>0 OR session.message_count>0))) LIMIT 1",
        &[session_id],
    ).await?;
    Ok(!rows.is_empty())
}

pub(super) async fn advance_cursor(
    store: &DatabaseSessionStore,
    session_id: &str,
    expected: Option<&agent_protocol::DreamMemoryCursor>,
    next: &agent_protocol::DreamMemoryCursor,
    covers_until_seq: Option<u64>,
) -> TalosResult<()> {
    let generation = i64::try_from(next.generation)
        .map_err(|error| {
            TalosError::internal_with_source("Store Dream Memory consumption generation.", error)
        })?
        .to_string();
    let offset = next.offset.to_string();
    let covered = i64::try_from(covers_until_seq.unwrap_or(0))
        .map_err(|error| {
            TalosError::internal_with_source("Store Dream Memory source coverage.", error)
        })?
        .to_string();
    let mut tx = store.storage.begin_transaction().await?;
    let changed = if let Some(expected) = expected {
        if expected.generation != next.generation || expected.offset > next.offset {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Dream Memory consumption cursor changed.",
            ));
        }
        tx.execute("UPDATE session_dream_cursors SET formation_offset=?,covered_until_source_seq=MAX(covered_until_source_seq,?) WHERE session_id=? AND generation=? AND formation_offset=?", &[&offset,&covered,session_id,&generation,&expected.offset.to_string()]).await?
    } else {
        tx.execute("INSERT OR IGNORE INTO session_dream_cursors(session_id,generation,formation_offset,covered_until_source_seq) VALUES(?,?,?,?)", &[session_id,&generation,&offset,&covered]).await?
    };
    if changed != 1 {
        return Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "Dream Memory consumption cursor changed.",
        ));
    }
    tx.commit().await
}
