//! Rebuild derived rows from bounded source pages without discarding product receipts.
use super::*;

pub struct WorkerProjectionRebuildParams {
    pub session_id: String,
    pub execution_node_id: String,
    pub history_generation: u64,
}

#[derive(Debug, PartialEq)]
pub struct WorkerProjectionRebuildResult {
    pub cursor: AgentSessionEventCursor,
    pub event_count: u64,
}

pub(in crate::session) async fn begin_rebuild(
    store: &DatabaseSessionStore,
    params: WorkerProjectionRebuildParams,
) -> TalosResult<String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut tx = store.storage.begin_transaction().await?;
    let rows = tx.query("SELECT execution_node_id,history_generation,execution_architecture FROM sessions WHERE session_key=?", &[&params.session_id]).await?;
    let row = rows
        .first()
        .ok_or_else(|| TalosError::new(ErrorCode::SessionNotFound, "Session is unavailable."))?;
    if row.get_string("execution_node_id").as_deref() != Some(&params.execution_node_id)
        || row.get_i64("history_generation") != i64::try_from(params.history_generation).ok()
        || row.get_string("execution_architecture").as_deref() != Some("worker")
    {
        return Err(conflict(
            "Execution view rebuilding requires the original Worker binding",
        ));
    }
    tx.execute(
        "DELETE FROM worker_projection_rebuilds WHERE session_id=?",
        &[&params.session_id],
    )
    .await?;
    tx.execute("INSERT INTO worker_projection_rebuilds(session_id,request_id,execution_node_id,history_generation) VALUES (?,?,?,?)", &[&params.session_id,&request_id,&params.execution_node_id,&params.history_generation.to_string()]).await?;
    tx.commit().await?;
    Ok(request_id)
}

pub(in crate::session) async fn stage_rebuild(
    store: &DatabaseSessionStore,
    request_id: &str,
    params: WorkerProjectionParams,
) -> TalosResult<()> {
    validate(&params)?;
    let mut tx = store.storage.begin_transaction().await?;
    let rows = tx.query("SELECT session_id,execution_node_id,history_generation,worker_offset,complete FROM worker_projection_rebuilds WHERE request_id=?", &[request_id]).await?;
    let row = rows
        .first()
        .ok_or_else(|| conflict("Execution view rebuild is no longer current"))?;
    let offset = row.get_i64("worker_offset").ok_or_else(corrupt)?;
    if row.get_string("session_id").as_deref() != Some(&params.session_id)
        || row.get_string("execution_node_id").as_deref() != Some(&params.execution_node_id)
        || row.get_i64("history_generation")
            != i64::try_from(params.page.next_cursor.generation).ok()
        || row.get_i64("complete") != Some(0)
        || params
            .after_cursor
            .as_ref()
            .map_or(0, |cursor| cursor.offset)
            != offset
    {
        return Err(conflict(
            "Execution view rebuild page does not follow its source position",
        ));
    }
    for fact in &params.page.events {
        tx.execute("INSERT INTO worker_projection_rebuild_events(request_id,worker_offset,fact_json) VALUES (?,?,?)", &[request_id,&fact.cursor.offset.to_string(),&encode(fact)?]).await?;
    }
    tx.execute(
        "UPDATE worker_projection_rebuilds SET worker_offset=?,complete=? WHERE request_id=?",
        &[
            &params.page.next_cursor.offset.to_string(),
            if params.page.has_more { "0" } else { "1" },
            request_id,
        ],
    )
    .await?;
    tx.commit().await
}

pub(in crate::session) async fn finish_rebuild(
    store: &DatabaseSessionStore,
    request_id: &str,
) -> TalosResult<WorkerProjectionRebuildResult> {
    let rows = store
        .storage
        .query(
            "SELECT session_id FROM worker_projection_rebuilds WHERE request_id=?",
            &[request_id],
        )
        .await?;
    let session_id = rows
        .first()
        .and_then(|row| row.get_string("session_id"))
        .ok_or_else(|| conflict("Execution view rebuild is no longer current"))?;
    let mut tx = store.begin_session_transaction(&session_id).await?;
    let rows = tx
        .query(
            "SELECT * FROM worker_projection_rebuilds WHERE request_id=?",
            &[request_id],
        )
        .await?;
    let staged = rows
        .first()
        .ok_or_else(|| conflict("Execution view rebuild is no longer current"))?;
    if staged.get_i64("complete") != Some(1) {
        return Err(conflict(
            "Execution view rebuild has not received the complete source",
        ));
    }
    let execution_node_id = staged.get_string("execution_node_id").ok_or_else(corrupt)?;
    let generation = staged
        .get_i64("history_generation")
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(corrupt)?;
    let offset = staged.get_i64("worker_offset").ok_or_else(corrupt)?;
    let cursor = AgentSessionEventCursor {
        session_id: session_id.clone(),
        generation,
        offset,
    };
    let (_, current) = projection_state(
        tx.as_mut(),
        &WorkerProjectionParams {
            session_id: session_id.clone(),
            execution_node_id: execution_node_id.clone(),
            after_cursor: None,
            page: AgentSessionEventsResult {
                events: vec![],
                next_cursor: cursor.clone(),
                has_more: false,
                active_turn: None,
            },
        },
    )
    .await?;
    if tx
        .query(
            "SELECT 1 FROM sessions WHERE session_key=? AND execution_architecture='worker'",
            &[&session_id],
        )
        .await?
        .is_empty()
    {
        return Err(conflict(
            "Execution view rebuilding requires the original Worker binding",
        ));
    }
    let generation_text = generation.to_string();
    let mapped = tx.query("SELECT MAX(worker_offset) AS offset FROM worker_event_projection WHERE session_id=? AND history_generation=?", &[&session_id, &generation_text]).await?;
    let mapped = mapped[0].get_i64("offset").unwrap_or(0);
    let current = current.max(mapped);
    if offset < current {
        return Err(conflict(
            "Execution view rebuild no longer covers the product source cursor",
        ));
    }
    if current == 0
        && offset > 0
        && tx
            .query(
                "SELECT 1 FROM worker_projection_cursors WHERE session_id=?",
                &[&session_id],
            )
            .await?
            .is_empty()
    {
        return Err(conflict(
            "Execution view rebuilding requires a retained product receipt ledger",
        ));
    }
    let uncovered = tx
        .query(
            "SELECT 1 FROM worker_event_projection p LEFT JOIN worker_projection_rebuild_events r
         ON r.request_id=?1 AND r.worker_offset=p.worker_offset
         WHERE p.session_id=?2 AND p.history_generation=?3 AND r.worker_offset IS NULL LIMIT 1",
            &[request_id, &session_id, &generation_text],
        )
        .await?;
    if !uncovered.is_empty() {
        return Err(TalosError::new(
            ErrorCode::EventCursorExpired,
            "Execution source history no longer covers the retained product receipts.",
        ));
    }
    if !tx.query(
        "SELECT 1 FROM agent_events e WHERE e.session_id=?1 AND e.history_generation=?2
         AND e.sequence IS NOT NULL AND NOT EXISTS (
             SELECT 1 FROM worker_event_projection p WHERE p.session_id=?1 AND p.history_generation=?2 AND p.product_event_seq=e.event_seq
         ) LIMIT 1",
        &[&session_id, &generation_text],
    ).await?.is_empty() {
        return Err(conflict("Execution view rebuilding cannot reconstruct a missing product receipt"));
    }
    if !tx.query(
        "SELECT 1 FROM worker_event_projection p JOIN agent_events e ON e.event_seq=p.product_event_seq
         WHERE p.session_id=? AND p.history_generation=? AND (e.session_id<>p.session_id OR e.history_generation<>p.history_generation OR e.sequence IS NULL) LIMIT 1",
        &[&session_id, &generation_text],
    ).await?.is_empty() {
        return Err(conflict("Execution source mapping does not identify a canonical product event"));
    }
    let mut after = 0;
    while let Some((fact, mapping)) = next_fact(
        tx.as_mut(),
        request_id,
        &session_id,
        &generation_text,
        after,
    )
    .await?
    {
        if let Some(mapping) = mapping {
            if mapping.get_i64("product_persisted_at").is_none() {
                return Err(conflict(
                    "Execution event has no retained original product timestamp",
                ));
            }
            // Turn starts also own product settlement watermarks. Source
            // facts cannot recreate those business values if the row is lost.
            if fact.event.sequence == 1 && tx.query(
                "SELECT 1 FROM agent_events WHERE event_seq=? AND session_id=? AND history_generation=? AND turn_id=? AND sequence=1",
                &[&mapping.get_i64("product_event_seq").ok_or_else(corrupt)?.to_string(), &session_id, &generation_text, &fact.event.identity.turn_id],
            ).await?.is_empty() {
                return Err(conflict("Execution view rebuilding requires the original Turn business metadata"));
            }
            validate_replayed_fact(tx.as_mut(), &mapping, &fact).await?;
        } else if fact.cursor.offset <= current {
            return Err(conflict(
                "Execution view rebuilding cannot reconstruct a missing product receipt",
            ));
        }
        after = fact.cursor.offset;
    }
    let cancellation = tx
        .query(
            "SELECT turn_id,cancellation_request_id FROM agent_active_turns WHERE session_id=?",
            &[&session_id],
        )
        .await?
        .into_iter()
        .next();
    // Keep each old start until its replacement is written so product-owned
    // augmentation can be copied one Turn at a time without an unbounded buffer.
    tx.execute(
        "DELETE FROM agent_events WHERE session_id=?1 AND history_generation=?2 AND sequence IS NOT NULL
         AND NOT EXISTS (
             SELECT 1 FROM worker_event_projection p JOIN worker_projection_rebuild_events r
             ON r.request_id=?3 AND r.worker_offset=p.worker_offset
             WHERE p.session_id=?1 AND p.history_generation=?2 AND p.product_event_seq=agent_events.event_seq
               AND json_extract(r.fact_json,'$.event.sequence')=1
         )",
        &[&session_id, &generation_text, request_id],
    ).await?;
    tx.execute(
        "DELETE FROM agent_active_turns WHERE session_id=?",
        &[&session_id],
    )
    .await?;
    tx.execute(
        "UPDATE sessions SET message_count=0,latest_turn_seq=0,usage_json=? WHERE session_key=?",
        &[&encode(&talos_types::SessionUsage::default())?, &session_id],
    )
    .await?;
    let mut count = 0;
    after = 0;
    while let Some((fact, mapping)) = next_fact(
        tx.as_mut(),
        request_id,
        &session_id,
        &generation_text,
        after,
    )
    .await?
    {
        let preserved_position = mapping
            .as_ref()
            .map(|row| -> TalosResult<PreservedEventPosition> {
                let event_seq = row
                    .get_i64("product_event_seq")
                    .filter(|value| *value > 0)
                    .ok_or_else(corrupt)?;
                let persisted_at = row.get_i64("product_persisted_at").ok_or_else(|| {
                    conflict("Execution event has no retained original product timestamp")
                })?;
                Ok(PreservedEventPosition {
                    event_seq,
                    persisted_at,
                })
            })
            .transpose()?;
        let saved = if fact.event.sequence == 1 {
            if let Some(position) = preserved_position {
                let sequence = position.event_seq;
                let saved = tx
                    .query(
                        "SELECT * FROM agent_events WHERE event_seq=?",
                        &[&sequence.to_string()],
                    )
                    .await?
                    .into_iter()
                    .next();
                tx.execute(
                    "DELETE FROM agent_events WHERE event_seq=?",
                    &[&sequence.to_string()],
                )
                .await?;
                saved
            } else {
                None
            }
        } else {
            None
        };
        let record = project_event(tx.as_mut(), &fact, preserved_position).await?;
        if let Some(saved) = saved {
            restore_start(tx.as_mut(), &record, &saved).await?;
        }
        if mapping.is_some() {
            metadata::restore(tx.as_mut(), &fact).await?;
        } else {
            metadata::project(tx.as_mut(), &fact, &record).await?;
            map_event(tx.as_mut(), &fact, &record).await?;
        }
        after = fact.cursor.offset;
        count += 1;
    }
    if let Some((turn, request)) = cancellation.and_then(|row| {
        row.get_string("turn_id")
            .zip(row.get_string("cancellation_request_id"))
    }) {
        tx.execute("UPDATE agent_active_turns SET cancellation_request_id=? WHERE session_id=? AND turn_id=?", &[&request, &session_id, &turn]).await?;
    }
    tx.execute("INSERT INTO worker_projection_cursors(session_id,execution_node_id,history_generation,worker_offset) VALUES (?,?,?,?) ON CONFLICT(session_id) DO UPDATE SET execution_node_id=excluded.execution_node_id,history_generation=excluded.history_generation,worker_offset=excluded.worker_offset", &[&session_id, &execution_node_id, &generation_text, &offset.to_string()]).await?;
    tx.execute(
        "DELETE FROM worker_projection_rebuilds WHERE request_id=?",
        &[request_id],
    )
    .await?;
    tx.commit().await?;
    Ok(WorkerProjectionRebuildResult {
        cursor,
        event_count: count,
    })
}

async fn next_fact(
    tx: &mut dyn StorageTransaction,
    request_id: &str,
    session_id: &str,
    generation: &str,
    after: i64,
) -> TalosResult<Option<(AgentCommittedEvent, Option<talos_storage::Row>)>> {
    let rows = tx.query(
        "SELECT r.fact_json,p.* FROM worker_projection_rebuild_events r LEFT JOIN worker_event_projection p
         ON p.session_id=?1 AND p.history_generation=?2 AND p.worker_offset=r.worker_offset
         WHERE r.request_id=?3 AND r.worker_offset>?4 ORDER BY r.worker_offset LIMIT 1",
        &[session_id, generation, request_id, &after.to_string()],
    ).await?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let fact = decode(&row.get_string("fact_json").ok_or_else(corrupt)?)?;
    let mapping = row.get_i64("product_event_seq").is_some().then_some(row);
    Ok(Some((fact, mapping)))
}

async fn restore_start(
    tx: &mut dyn StorageTransaction,
    event: &AgentEventRecord,
    saved: &talos_storage::Row,
) -> TalosResult<()> {
    tx.execute(
        "UPDATE agent_events SET execution_capture_json=NULLIF(?,''),automation_origin_json=NULLIF(?,''),
         automation_input_message_id=NULLIF(?,''),automation_accounted_tokens=?,automation_loop_decision_json=NULLIF(?,'') WHERE event_seq=?",
        &[&saved.get_string("execution_capture_json").unwrap_or_default(), &saved.get_string("automation_origin_json").unwrap_or_default(),
          &saved.get_string("automation_input_message_id").unwrap_or_default(), &saved.get_i64("automation_accounted_tokens").ok_or_else(corrupt)?.to_string(),
          &saved.get_string("automation_loop_decision_json").unwrap_or_default(), &event.event_seq.to_string()],
    ).await?;
    Ok(())
}
