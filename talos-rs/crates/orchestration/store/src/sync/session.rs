use super::{codec, transcode, StoreSyncAdapter};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use talos_sync::{SyncEventKey, SyncEventMapping, SyncImportBatch};
use talos_sync_types::{SyncChange, SyncSessionSnapshot, SyncTimelineEntry};

pub(crate) async fn key_at(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    seq: i64,
) -> TalosResult<Option<SyncEventKey>> {
    if seq == 0 {
        return Ok(None);
    }
    let generation = adapter
        .generation(
            tx,
            &talos_sync::SyncResourceKey::Session {
                session_id: session_id.into(),
            },
        )
        .await?;
    let mapped = tx.query("SELECT stable_event_key FROM sync_event_map WHERE hub_id = ? AND account_id = ? AND session_id = ? AND generation = ? AND local_event_seq = ?", &[&adapter.binding.scope.hub_id, &adapter.binding.scope.account_id, session_id, &generation.to_string(), &seq.to_string()]).await?;
    if let Some(raw) = mapped
        .first()
        .and_then(|r| r.get_string("stable_event_key"))
    {
        return serde_json::from_str(&raw).map(Some).map_err(codec);
    }
    let rows = tx.query("SELECT session_id, turn_id, sequence FROM agent_events WHERE session_id = ? AND event_seq = ?", &[session_id, &seq.to_string()]).await?;
    Ok(rows.first().and_then(|row| {
        row.get_i64("sequence").map(|sequence| SyncEventKey::Agent {
            identity: talos_types::AgentTurnRef::new(
                session_id,
                row.get_string("turn_id").expect("persisted Turn ID"),
            ),
            sequence: sequence as u64,
        })
    }))
}

pub(super) async fn apply_metadata(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
    snapshot: &SyncSessionSnapshot,
) -> TalosResult<()> {
    let session = &snapshot.session;
    let local = adapter.local_node(&snapshot.binding.computer_id);
    let rows = tx
        .query(
            "SELECT * FROM sessions WHERE session_key = ?",
            &[&session.id],
        )
        .await?;
    if let Some(row) = rows.first() {
        if row.get_string("execution_node_id").as_deref() != Some(&local)
            || row.get_string("agent_id").as_deref() != Some(&snapshot.binding.agent_id)
            || row.get_string("workspace_id").as_deref() != Some(&snapshot.binding.workspace_id)
            || row
                .get_string("workspace_user_id")
                .or_else(|| row.get_string("user_id"))
                .as_deref()
                != Some(&snapshot.binding.workspace_user_id)
            || ![&adapter.binding.source_user_id, &batch.scope.account_id]
                .contains(&&row.get_string("user_id").unwrap_or_default())
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Session identity has a different immutable Worker binding.",
            ));
        }
        let record = crate::session::DatabaseSessionStore::parse_session_record(row)?;
        if transcode::<_, talos_sync_types::generated::SessionTreeIdentity>(&record.tree_identity)?
            != snapshot.tree_identity
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Session tree identity cannot change.",
            ));
        }
    }
    let (parent_session, parent_turn, root, path) = match &snapshot.tree_identity {
        talos_sync_types::generated::SessionTreeIdentity::Primary => (None, None, None, None),
        talos_sync_types::generated::SessionTreeIdentity::Subagent {
            parent,
            tree_root_session_id,
            path,
        } => {
            if tx.query("SELECT 1 FROM sessions WHERE session_key = ? AND execution_node_id = ? AND user_id IN (?, ?)", &[&parent.session_id, &local, &adapter.binding.source_user_id, &batch.scope.account_id]).await?.is_empty() { return Err(TalosError::new(ErrorCode::ServiceUnavailable, "The synchronized parent Session is not yet available.")); }
            (
                Some(parent.session_id.as_str()),
                Some(parent.turn_id.as_str()),
                Some(tree_root_session_id.as_str()),
                Some(path.as_str()),
            )
        }
    };
    let source_type = serde_json::to_value(session.source_type)
        .map_err(codec)?
        .as_str()
        .expect("original source type")
        .to_owned();
    let source_provider = serde_json::to_value(session.source_provider)
        .map_err(codec)?
        .as_str()
        .expect("original provider")
        .to_owned();
    let conf = serde_json::to_string(&snapshot.conf).map_err(codec)?;
    let environment = serde_json::to_string(&snapshot.environment).map_err(codec)?;
    let usage = serde_json::to_string(&session.usage).map_err(codec)?;
    tx.execute(r#"INSERT INTO sessions(session_key, agent_id, execution_node_id, workspace_id, workspace_user_id, user_id, title, source_type, source_provider, channel_id, thread_id, environment_json, conf_json, current_directory, project_id, created_at, last_activity, message_count, latest_turn_seq, memory_processed_until_event_seq, usage_json, parent_session_id, parent_turn_id, tree_root_session_id, agent_path)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULLIF(?, ''), NULLIF(?, ''), ?, ?, NULLIF(?, ''), NULLIF(?, ''), ?, ?, ?, 0, 0, ?, NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''))
        ON CONFLICT(session_key) DO UPDATE SET title = excluded.title, conf_json = excluded.conf_json, current_directory = excluded.current_directory, last_activity = excluded.last_activity, message_count = excluded.message_count, usage_json = excluded.usage_json"#, &[&session.id, &snapshot.binding.agent_id, &local, &snapshot.binding.workspace_id, &snapshot.binding.workspace_user_id, &batch.scope.account_id, &session.title, &source_type, &source_provider, snapshot.channel_id.as_deref().unwrap_or_default(), snapshot.thread_id.as_deref().unwrap_or_default(), &environment, &conf, session.current_directory.as_deref().unwrap_or_default(), session.project_id.as_deref().unwrap_or_default(), &session.created_at.to_string(), &session.last_activity.to_string(), &session.message_count.to_string(), &usage, parent_session.unwrap_or_default(), parent_turn.unwrap_or_default(), root.unwrap_or_default(), path.unwrap_or_default()]).await?;
    Ok(())
}

pub(super) async fn apply(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
) -> TalosResult<()> {
    let session_id = batch.resource.id();
    let generation = batch.version.history_generation;
    // Every local generation's map is preserved until reset removes its facts.
    let old = tx.query("SELECT MIN(generation) AS generation FROM sync_event_map WHERE hub_id = ? AND account_id = ? AND session_id = ?", &[&batch.scope.hub_id, &batch.scope.account_id, session_id]).await?;
    if old
        .first()
        .and_then(|r| r.get_i64("generation"))
        .is_some_and(|g| (g as u64) < generation)
    {
        if !tx
            .query(
                "SELECT 1 FROM agent_active_turns WHERE session_id = ?",
                &[session_id],
            )
            .await?
            .is_empty()
        {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Cannot reset locally executing Session facts.",
            ));
        }
        tx.execute(
            "DELETE FROM agent_events WHERE session_id = ?",
            &[session_id],
        )
        .await?;
        tx.execute(
            "DELETE FROM agent_command_idempotency WHERE session_id = ?",
            &[session_id],
        )
        .await?;
        tx.execute(
            "DELETE FROM sync_event_map WHERE hub_id = ? AND account_id = ? AND session_id = ?",
            &[&batch.scope.hub_id, &batch.scope.account_id, session_id],
        )
        .await?;
    }
    tx.execute(
        "UPDATE sessions SET history_generation = ? WHERE session_key = ?",
        &[&generation.to_string(), session_id],
    )
    .await?;
    let ordered = ordered_events(&batch.changes)?;
    for entry in ordered {
        import_event(adapter, tx, batch, entry).await?;
    }
    let latest_turn_seq = batch.changes.iter().filter(|c| matches!(c, SyncChange::Event(e) if matches!(&e.event, talos_sync_types::generated::TimelineEventPayload::Agent(a) if matches!(a.payload, talos_sync_types::generated::AgentEventPayload::TurnCompleted(_))))).count();
    tx.execute(
        "UPDATE sessions SET latest_turn_seq = ? WHERE session_key = ?",
        &[&latest_turn_seq.to_string(), session_id],
    )
    .await?;
    let mut changes = batch.changes.iter().collect::<Vec<_>>();
    // Reused idempotency keys follow the canonical committed event order.
    // They retain each original request ID in the shared append-only facts.
    let mut command_sequences = std::collections::HashMap::new();
    for change in &changes {
        if let SyncChange::AcceptedCommand(command) = change {
            command_sequences.insert(
                command.request_id.as_str(),
                local_seq(tx, batch, &command.committed_event)
                    .await?
                    .ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::Conflict,
                            "Accepted command depends on missing synchronized facts.",
                        )
                    })?,
            );
        }
    }
    changes.sort_by_key(|change| match change {
        SyncChange::AcceptedCommand(command) => command_sequences[command.request_id.as_str()],
        _ => 0,
    });
    for change in changes {
        if let SyncChange::Metadata { after, .. } = change {
            if let talos_sync_types::SyncResourceSnapshot::Session(snapshot) = after.as_ref() {
                let through = match &snapshot.memory_processed_through {
                    Some(key) => local_seq(tx, batch, key).await?.ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::Conflict,
                            "Memory progress depends on missing synchronized facts.",
                        )
                    })?,
                    None => 0,
                };
                tx.execute("UPDATE sessions SET memory_processed_until_event_seq = ? WHERE session_key = ?", &[&through.to_string(), session_id]).await?;
            }
        }
        if let SyncChange::AcceptedCommand(command) = change {
            let seq = local_seq(tx, batch, &command.committed_event)
                .await?
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::Conflict,
                        "Accepted command depends on missing synchronized facts.",
                    )
                })?;
            let command_name = match command.command {
                talos_sync_types::generated::IdempotentCommand::TurnSubmission => "turn_submission",
                talos_sync_types::generated::IdempotentCommand::TurnInput => "turn_input",
            };
            let outcome = match command.status {
                talos_sync_types::generated::TurnCommandStatus::Pending => {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Pending command is not a completed synchronization fact.",
                    ))
                }
                talos_sync_types::generated::TurnCommandStatus::Accepted => "accepted",
                talos_sync_types::generated::TurnCommandStatus::Failed => "failed",
                talos_sync_types::generated::TurnCommandStatus::FailedNoFirstEvent => {
                    "failed_no_first_event"
                }
            };
            let current = tx.query("SELECT request_id, request_hash, turn_id, status, committed_event_seq, record_expires_at FROM agent_command_idempotency WHERE session_id = ? AND command = ? AND idempotency_key = ?", &[session_id, command_name, &command.idempotency_key]).await?;
            let persisted_at = tx
                .query(
                    "SELECT persisted_at FROM agent_events WHERE session_id = ? AND event_seq = ?",
                    &[session_id, &seq.to_string()],
                )
                .await?
                .first()
                .and_then(|r| r.get_i64("persisted_at"))
                .ok_or_else(|| TalosError::internal("Accepted command event is unavailable."))?;
            if let Some(row) = current.first() {
                let same_request = row.get_string("request_id").as_deref()
                    == Some(&command.request_id)
                    && row.get_string("request_hash").as_deref() == Some(&command.request_hash)
                    && row.get_string("turn_id").as_deref() == Some(&command.identity.turn_id);
                if !same_request {
                    if row
                        .get_i64("committed_event_seq")
                        .is_some_and(|current| current >= seq)
                    {
                        continue;
                    }
                    if row
                        .get_i64("record_expires_at")
                        .is_none_or(|expiry| expiry > persisted_at)
                    {
                        return Err(TalosError::new(
                            ErrorCode::IdempotencyConflict,
                            "Synchronized command conflicts with an existing request.",
                        ));
                    }
                }
            }
            tx.execute("INSERT INTO agent_command_idempotency(session_id, command, idempotency_key, request_id, request_hash, turn_id, message_id, status, outcome, committed_event_seq, created_at, updated_at, record_expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'completed', ?, ?, ?, ?, NULLIF(?, '')) ON CONFLICT(session_id, command, idempotency_key) DO UPDATE SET request_id = excluded.request_id, request_hash = excluded.request_hash, turn_id = excluded.turn_id, message_id = excluded.message_id, status = 'completed', outcome = excluded.outcome, committed_event_seq = excluded.committed_event_seq, updated_at = excluded.updated_at, record_expires_at = excluded.record_expires_at, lease_id = NULL, lease_expires_at = NULL", &[session_id, command_name, &command.idempotency_key, &command.request_id, &command.request_hash, &command.identity.turn_id, &command.message_id, outcome, &seq.to_string(), &persisted_at.to_string(), &persisted_at.to_string(), &command.expires_at.map(|n| n.to_string()).unwrap_or_default()]).await?;
        }
    }
    Ok(())
}

fn ordered_events(changes: &[SyncChange]) -> TalosResult<Vec<&SyncTimelineEntry>> {
    let turns = changes
        .iter()
        .filter_map(|c| {
            if let SyncChange::Turn(t) = c {
                Some(t)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut order = Vec::new();
    let mut previous = None;
    while order.len() < turns.len() {
        let next = turns
            .iter()
            .filter(|t| t.previous_turn_id.as_deref() == previous)
            .collect::<Vec<_>>();
        if next.len() != 1 {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Synchronized Turn predecessor chain is incomplete or branched.",
            ));
        }
        previous = Some(next[0].identity.turn_id.as_str());
        order.push(previous.expect("Turn identity"));
    }
    let mut events = changes
        .iter()
        .filter_map(|c| {
            if let SyncChange::Event(e) = c {
                Some(e)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    events.sort_by_key(|e| {
        let (turn, sequence) = match &e.key {
            talos_sync_types::SyncEventKey::Agent { identity, sequence } => {
                (identity.turn_id.as_str(), *sequence)
            }
            talos_sync_types::SyncEventKey::Notice { .. } => match &e.event {
                talos_sync_types::generated::TimelineEventPayload::SystemNotice(n) => {
                    (n.identity.turn_id.as_str(), u64::MAX)
                }
                _ => ("", u64::MAX),
            },
        };
        (
            order
                .iter()
                .position(|id| *id == turn)
                .unwrap_or(usize::MAX),
            sequence,
            e.persisted_at,
        )
    });
    Ok(events)
}

pub(super) async fn local_seq(
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
    key: &talos_sync_types::SyncEventKey,
) -> TalosResult<Option<i64>> {
    let key = serde_json::to_string(key).map_err(codec)?;
    Ok(tx.query("SELECT local_event_seq FROM sync_event_map WHERE hub_id = ? AND account_id = ? AND session_id = ? AND generation = ? AND stable_event_key = ?", &[&batch.scope.hub_id, &batch.scope.account_id, batch.resource.id(), &batch.version.history_generation.to_string(), &key]).await?.first().and_then(|r| r.get_i64("local_event_seq")))
}

async fn import_event(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
    entry: &SyncTimelineEntry,
) -> TalosResult<()> {
    if let Some(seq) = local_seq(tx, batch, &entry.key).await? {
        let row = tx
            .query(
                "SELECT * FROM agent_events WHERE event_seq = ? AND session_id = ?",
                &[&seq.to_string(), batch.resource.id()],
            )
            .await?;
        let existing = row
            .first()
            .map(crate::session::agent_execution::notice::parse)
            .transpose()?
            .ok_or_else(|| TalosError::internal("Synchronized event cursor has no fact."))?;
        if transcode::<_, talos_sync_types::generated::TimelineEventPayload>(&existing.event)?
            != entry.event
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Stable event identity has different contents.",
            ));
        }
        return Ok(());
    }
    let event: talos_agent_api::TimelineEventPayload = transcode(&entry.event)?;
    let (identity, sequence, occurred, payload) = match &event {
        talos_agent_api::TimelineEventPayload::Agent(e) => (
            &e.identity,
            Some(e.sequence),
            e.occurred_at,
            serde_json::to_string(&e.payload).map_err(codec)?,
        ),
        talos_agent_api::TimelineEventPayload::SystemNotice(e) => (
            &e.identity,
            None,
            e.occurred_at,
            serde_json::to_string(&e.notice).map_err(codec)?,
        ),
    };
    let rows = tx.query("INSERT INTO agent_events(session_id, turn_id, sequence, occurred_at, persisted_at, payload_json) VALUES (?, ?, NULLIF(?, ''), ?, ?, ?) RETURNING event_seq", &[&identity.session_id, &identity.turn_id, &sequence.map(|n| n.to_string()).unwrap_or_default(), &occurred.to_string(), &entry.persisted_at.to_string(), &payload]).await?;
    let seq = super::metadata::number(&rows[0], "event_seq")?;
    adapter
        .sync
        .map_event(
            tx,
            SyncEventMapping {
                scope: batch.scope.clone(),
                session_id: batch.resource.id().into(),
                generation: batch.version.history_generation,
                key: transcode(&entry.key)?,
                local_event_id: talos_types::TimelineEventId::from_seq(seq).to_string(),
                local_event_seq: seq,
            },
        )
        .await
}
