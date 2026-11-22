use std::collections::HashMap;

use super::{codec, StoreSyncAdapter};
use talos_agent_api::{AgentEventPayload, TimelineEventPayload};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
use talos_sync::{
    SyncChange, SyncEventKey, SyncImportBatch, SyncResourceSnapshot, SyncSessionSnapshot,
    SyncTimelineEntry,
};

pub(crate) async fn key_at(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    seq: i64,
) -> TalosResult<Option<SyncEventKey>> {
    let generation = adapter
        .generation(
            tx,
            &talos_sync::SyncResourceKey::Session {
                session_id: session_id.into(),
            },
        )
        .await?;
    key_at_generation(adapter, tx, session_id, generation, seq).await
}

pub(super) async fn key_at_generation(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    generation: u64,
    seq: i64,
) -> TalosResult<Option<SyncEventKey>> {
    if seq == 0 {
        return Ok(None);
    }
    let mapped = tx.query("SELECT stable_event_key FROM visible_sync_event_map WHERE hub_id = ? AND account_id = ? AND session_id = ? AND generation = ? AND local_event_seq = ?", &[&adapter.binding.scope.hub_id, &adapter.binding.scope.account_id, session_id, &generation.to_string(), &seq.to_string()]).await?;
    if let Some(raw) = mapped
        .first()
        .and_then(|r| r.get_string("stable_event_key"))
    {
        return serde_json::from_str(&raw).map(Some).map_err(codec);
    }
    let rows = tx.query("SELECT session_id, turn_id, sequence FROM committed_agent_events WHERE session_id = ? AND history_generation = ? AND event_seq = ?", &[session_id, &generation.to_string(), &seq.to_string()]).await?;
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
        if record.tree_identity != snapshot.tree_identity {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Session tree identity cannot change.",
            ));
        }
    }
    let (parent_session, parent_turn, root, path) = match &snapshot.tree_identity {
        talos_types::SessionTreeIdentity::Primary => (None, None, None, None),
        talos_types::SessionTreeIdentity::Subagent {
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
    let source_type = session.source_type.as_str();
    let source_provider = serde_json::to_value(&session.source_provider).map_err(codec)?;
    let source_provider = source_provider.as_str().expect("original provider");
    let conf = serde_json::to_string(&snapshot.conf).map_err(codec)?;
    let environment = serde_json::to_string(&snapshot.environment).map_err(codec)?;
    let usage = serde_json::to_string(&session.usage).map_err(codec)?;
    let current_directory = match session.current_directory.as_ref() {
        Some(path) => path
            .to_str()
            .ok_or_else(|| TalosError::internal("Synchronized Session directory must be UTF-8."))?,
        None => "",
    };
    tx.execute(r#"INSERT INTO sessions(session_key, session_kind, agent_id, execution_node_id, workspace_id, workspace_user_id, user_id, title, source_type, source_provider, channel_id, thread_id, environment_json, conf_json, current_directory, project_id, created_at, last_activity, message_count, latest_turn_seq, memory_processed_until_event_seq, usage_json, parent_session_id, parent_turn_id, tree_root_session_id, agent_path)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULLIF(?, ''), NULLIF(?, ''), ?, ?, NULLIF(?, ''), NULLIF(?, ''), ?, ?, ?, 0, 0, ?, NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''), NULLIF(?, ''))
        ON CONFLICT(session_key) DO UPDATE SET session_kind = CASE WHEN sessions.session_kind = 'talo' THEN sessions.session_kind ELSE excluded.session_kind END, title = excluded.title, conf_json = excluded.conf_json, current_directory = excluded.current_directory, last_activity = excluded.last_activity, message_count = excluded.message_count, usage_json = excluded.usage_json"#, &[&session.id, session.session_kind.as_str(), &snapshot.binding.agent_id, &local, &snapshot.binding.workspace_id, &snapshot.binding.workspace_user_id, &batch.scope.account_id, &session.title, source_type, source_provider, snapshot.channel_id.as_deref().unwrap_or_default(), snapshot.thread_id.as_deref().unwrap_or_default(), &environment, &conf, current_directory, session.project_id.as_deref().unwrap_or_default(), &session.created_at.to_string(), &session.last_activity.to_string(), &session.message_count.to_string(), &usage, parent_session.unwrap_or_default(), parent_turn.unwrap_or_default(), root.unwrap_or_default(), path.unwrap_or_default()]).await?;
    Ok(())
}

pub(super) async fn apply(
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
) -> TalosResult<()> {
    let session_id = batch.resource.id();
    let generation = batch.version.history_generation;
    let old = tx
        .query(
            "SELECT history_generation AS generation FROM sessions WHERE session_key=?",
            &[session_id],
        )
        .await?;
    if old
        .first()
        .and_then(|r| r.get_i64("generation"))
        .is_some_and(|g| (g as u64) < generation)
    {
        super::source::retire_history(tx, session_id, /*deleted*/ false).await?;
        tx.execute(
            "DELETE FROM agent_command_idempotency WHERE session_id = ?",
            &[session_id],
        )
        .await?;
    }
    tx.execute(
        "UPDATE sessions SET history_generation = ? WHERE session_key = ?",
        &[&generation.to_string(), session_id],
    )
    .await?;
    let latest_turn_seq = batch.changes.iter().filter(|c| matches!(c, SyncChange::Event(e) if matches!(&e.event, TimelineEventPayload::Agent(a) if matches!(a.payload, AgentEventPayload::TurnCompleted(_))))).count();
    tx.execute(
        "UPDATE sessions SET latest_turn_seq = ? WHERE session_key = ?",
        &[&latest_turn_seq.to_string(), session_id],
    )
    .await?;
    // Reused idempotency keys follow the canonical committed event order.
    // They retain each original request ID in the shared append-only facts.
    let mut commands = Vec::new();
    for change in &batch.changes {
        if let SyncChange::AcceptedCommand(command) = change {
            let seq = local_seq(tx, batch, &command.committed_event)
                .await?
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::Conflict,
                        "Accepted command depends on missing synchronized facts.",
                    )
                })?;
            commands.push((seq, command));
        }
        if let SyncChange::Metadata { after, .. } = change {
            if let SyncResourceSnapshot::Session(snapshot) = after.as_ref() {
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
    }
    commands.sort_by_key(|(seq, _)| *seq);
    for (seq, command) in commands {
        let command_name = command.command.as_str();
        let outcome = match command.status {
            talos_types::TurnCommandStatus::Pending => {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Pending command is not a completed synchronization fact.",
                ))
            }
            talos_types::TurnCommandStatus::Accepted => "accepted",
            talos_types::TurnCommandStatus::Failed => "failed",
            talos_types::TurnCommandStatus::FailedNoFirstEvent => "failed_no_first_event",
        };
        let current = tx.query("SELECT request_id, request_hash, turn_id, status, committed_event_seq, record_expires_at FROM agent_command_idempotency WHERE session_id = ? AND command = ? AND idempotency_key = ?", &[session_id, command_name, &command.idempotency_key]).await?;
        let persisted_at = tx
                .query(
                    "SELECT persisted_at FROM visible_agent_events WHERE session_id = ? AND event_seq = ?",
                    &[session_id, &seq.to_string()],
                )
                .await?
                .first()
                .and_then(|r| r.get_i64("persisted_at"))
                .ok_or_else(|| TalosError::internal("Accepted command event is unavailable."))?;
        if let Some(row) = current.first() {
            let same_request = row.get_string("request_id").as_deref() == Some(&command.request_id)
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
    Ok(())
}

pub(super) fn ordered_events(changes: &[SyncChange]) -> TalosResult<Vec<&SyncTimelineEntry>> {
    let mut children = std::collections::HashMap::new();
    let invalid_chain = || {
        TalosError::new(
            ErrorCode::Conflict,
            "Synchronized Turn predecessor chain is incomplete or branched.",
        )
    };
    for change in changes {
        if let SyncChange::Turn(turn) = change {
            let previous = turn.previous_turn_id.as_deref();
            if children
                .insert(previous, turn.identity.turn_id.as_str())
                .is_some()
            {
                return Err(invalid_chain());
            }
        }
    }
    let mut order = std::collections::HashMap::with_capacity(children.len());
    let mut previous = None;
    while let Some(turn) = children.remove(&previous) {
        order.insert(turn, order.len());
        previous = Some(turn);
    }
    if !children.is_empty() {
        return Err(invalid_chain());
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
            SyncEventKey::Agent { identity, sequence } => (identity.turn_id.as_str(), *sequence),
            SyncEventKey::Notice { .. } => match &e.event {
                TimelineEventPayload::SystemNotice(n) => (n.identity.turn_id.as_str(), u64::MAX),
                _ => ("", u64::MAX),
            },
        };
        (
            order.get(turn).copied().unwrap_or(usize::MAX),
            sequence,
            e.persisted_at,
        )
    });
    Ok(events)
}

async fn local_seq(
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
    key: &SyncEventKey,
) -> TalosResult<Option<i64>> {
    let key = serde_json::to_string(key).map_err(codec)?;
    Ok(tx.query("SELECT local_event_seq FROM visible_sync_event_map WHERE hub_id = ? AND account_id = ? AND session_id = ? AND generation = ? AND stable_event_key = ?", &[&batch.scope.hub_id, &batch.scope.account_id, batch.resource.id(), &batch.version.history_generation.to_string(), &key]).await?.first().and_then(|r| r.get_i64("local_event_seq")))
}

pub(super) async fn new_events(
    tx: &mut dyn StorageTransaction,
    batch: &SyncImportBatch,
    entries: &[&SyncTimelineEntry],
) -> TalosResult<Vec<ImportEvent>> {
    let keys = entries
        .iter()
        .map(|entry| serde_json::to_string(&entry.key).map_err(codec))
        .collect::<TalosResult<Vec<_>>>()?;
    let keys_json = serde_json::to_string(&keys).map_err(codec)?;
    let existing = tx
        .query(
            "SELECT m.stable_event_key, e.event_seq, e.session_id, e.turn_id, e.sequence,
                e.occurred_at, e.persisted_at, e.payload_json
         FROM visible_sync_event_map m
         LEFT JOIN committed_agent_events e
           ON e.event_seq = m.local_event_seq AND e.session_id = m.session_id
         WHERE m.hub_id = ? AND m.account_id = ? AND m.session_id = ? AND m.generation = ?
           AND m.stable_event_key IN (SELECT value FROM json_each(?))",
            &[
                &batch.scope.hub_id,
                &batch.scope.account_id,
                batch.resource.id(),
                &batch.version.history_generation.to_string(),
                &keys_json,
            ],
        )
        .await?;
    let existing = existing
        .into_iter()
        .map(|row| Ok((super::metadata::text(&row, "stable_event_key")?, row)))
        .collect::<TalosResult<HashMap<_, _>>>()?;
    let mut missing = Vec::new();
    for (entry, key) in entries.iter().zip(keys) {
        if let Some(row) = existing.get(&key) {
            verify_existing_event(entry, row)?;
        } else {
            missing.push(ImportEvent::from_entry(entry)?);
        }
    }
    Ok(missing)
}

fn verify_existing_event(entry: &SyncTimelineEntry, row: &Row) -> TalosResult<()> {
    if row.get_i64("event_seq").is_none() {
        return Err(TalosError::internal(
            "Synchronized event cursor has no fact.",
        ));
    }
    let existing = crate::session::agent_execution::notice::parse(row)?;
    if existing.event != entry.event {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "Stable event identity has different contents.",
        ));
    }
    Ok(())
}

pub(super) struct ImportEvent {
    identity: talos_types::AgentTurnRef,
    sequence: String,
    occurred_at: i64,
    persisted_at: i64,
    payload: String,
    key: String,
}

impl ImportEvent {
    fn from_entry(entry: &SyncTimelineEntry) -> TalosResult<Self> {
        let (identity, sequence, occurred_at, payload) = match &entry.event {
            TimelineEventPayload::Agent(e) => (
                e.identity.clone(),
                e.sequence.to_string(),
                e.occurred_at,
                serde_json::to_string(&e.payload).map_err(codec)?,
            ),
            TimelineEventPayload::SystemNotice(e) => (
                e.identity.clone(),
                String::new(),
                e.occurred_at,
                serde_json::to_string(&e.notice).map_err(codec)?,
            ),
        };
        Ok(Self {
            identity,
            sequence,
            occurred_at,
            persisted_at: entry.persisted_at,
            payload,
            key: serde_json::to_string(&entry.key).map_err(codec)?,
        })
    }

    pub(super) fn bytes(&self) -> usize {
        self.payload.len() + self.key.len()
    }

    pub(super) async fn insert(
        &self,
        tx: &mut dyn StorageTransaction,
        batch: &SyncImportBatch,
        import_id: i64,
    ) -> TalosResult<()> {
        let rows = tx.query(
            "INSERT INTO agent_events(session_id, turn_id, sequence, occurred_at, persisted_at, payload_json, history_generation, import_id)
             VALUES (?, ?, NULLIF(?, ''), ?, ?, ?, ?, ?) RETURNING event_seq",
            &[&self.identity.session_id, &self.identity.turn_id, &self.sequence, &self.occurred_at.to_string(), &self.persisted_at.to_string(), &self.payload, &batch.version.history_generation.to_string(), &import_id.to_string()],
        ).await?;
        let seq = super::metadata::number(&rows[0], "event_seq")?;
        tx.execute(
            "INSERT INTO sync_event_map(hub_id, account_id, session_id, generation, stable_event_key, local_event_id, local_event_seq, import_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            &[&batch.scope.hub_id, &batch.scope.account_id, batch.resource.id(), &batch.version.history_generation.to_string(), &self.key, &talos_types::TimelineEventId::from_seq(seq).to_string(), &seq.to_string(), &import_id.to_string()],
        ).await?;
        Ok(())
    }
}
