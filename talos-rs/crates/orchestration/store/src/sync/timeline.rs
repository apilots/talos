use super::{
    metadata::{number, text},
    StoreSyncAdapter,
};
use talos_agent_api::{AgentEventPayload, TimelineEventPayload};
use talos_error::TalosResult;
use talos_storage::StorageTransaction;
use talos_sync::{
    SyncAcceptedCommand, SyncActiveTurnMetadata, SyncChange, SyncEventKey, SyncEventMapping,
    SyncResourceKey, SyncTimelineEntry, SyncTurnMetadata,
};

pub(super) async fn tail(
    tx: &mut dyn StorageTransaction,
    resource: &SyncResourceKey,
) -> TalosResult<i64> {
    if !matches!(resource, SyncResourceKey::Session { .. }) {
        return Ok(0);
    }
    Ok(tx
        .query(
            "SELECT MAX(event_seq) AS tail FROM agent_events WHERE session_id = ?",
            &[resource.id()],
        )
        .await?
        .first()
        .and_then(|r| r.get_i64("tail"))
        .unwrap_or(0))
}

pub(super) async fn export(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    after: i64,
) -> TalosResult<Vec<SyncChange>> {
    let resource = SyncResourceKey::Session {
        session_id: session_id.into(),
    };
    let generation = adapter.generation(tx, &resource).await?;
    let mut changes = Vec::new();
    let mut cursor = after;
    loop {
        let rows = tx.query("SELECT event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json FROM agent_events WHERE session_id = ? AND event_seq > ? ORDER BY event_seq LIMIT 128", &[session_id, &cursor.to_string()]).await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let record = crate::session::agent_execution::notice::parse(&row)?;
            cursor = record.event_seq;
            let key = match &record.event {
                TimelineEventPayload::Agent(event) => SyncEventKey::Agent {
                    identity: event.identity.clone(),
                    sequence: event.sequence,
                },
                TimelineEventPayload::SystemNotice(_) => {
                    super::session::key_at(adapter, tx, session_id, cursor)
                        .await?
                        .unwrap_or_else(|| SyncEventKey::Notice {
                            peer_id: adapter.binding.peer_id.clone(),
                            source_event_id: talos_types::TimelineEventId::from_seq(cursor)
                                .to_string(),
                        })
                }
            };
            if let TimelineEventPayload::Agent(event) = &record.event {
                if matches!(event.payload, AgentEventPayload::TurnStarted(_)) {
                    let previous = tx.query("SELECT turn_id FROM agent_events WHERE session_id = ? AND event_seq < ? AND sequence = 1 ORDER BY event_seq DESC LIMIT 1", &[session_id, &cursor.to_string()]).await?.first().and_then(|r| r.get_string("turn_id"));
                    changes.push(SyncChange::Turn(SyncTurnMetadata {
                        identity: event.identity.clone(),
                        coordinator_id: adapter.binding.peer_id.clone(),
                        previous_turn_id: previous,
                    }));
                }
            }
            adapter
                .sync
                .map_event(
                    tx,
                    SyncEventMapping {
                        scope: adapter.binding.scope.clone(),
                        session_id: session_id.into(),
                        generation,
                        key: key.clone(),
                        local_event_id: talos_types::TimelineEventId::from_seq(cursor).to_string(),
                        local_event_seq: cursor,
                    },
                )
                .await?;
            changes.push(SyncChange::Event(SyncTimelineEntry {
                key,
                event: record.event,
                persisted_at: record.persisted_at,
            }));
        }
    }
    if let Some(active) = active_metadata(adapter, tx, session_id).await? {
        changes.push(SyncChange::ActiveTurnMetadata(active));
    }
    for row in tx.query("SELECT * FROM agent_command_idempotency WHERE session_id = ? AND status = 'completed' AND committed_event_seq > ? ORDER BY committed_event_seq", &[session_id, &after.to_string()]).await? {
        let identity = talos_types::AgentTurnRef::new(session_id, text(&row, "turn_id")?);
        if let Some(committed_event) = super::session::key_at(adapter, tx, session_id, number(&row, "committed_event_seq")?).await? {
            let command = match text(&row, "command")?.as_str() { "turn_submission" => talos_types::IdempotentCommand::TurnSubmission, "turn_input" => talos_types::IdempotentCommand::TurnInput, _ => return Err(talos_error::TalosError::internal("Invalid persisted command.")) };
            let status = match text(&row, "outcome")?.as_str() { "accepted" => talos_types::TurnCommandStatus::Accepted, "failed" => talos_types::TurnCommandStatus::Failed, "failed_no_first_event" => talos_types::TurnCommandStatus::FailedNoFirstEvent, _ => return Err(talos_error::TalosError::internal("Invalid persisted command outcome.")) };
            changes.push(SyncChange::AcceptedCommand(SyncAcceptedCommand { command, idempotency_key: text(&row, "idempotency_key")?, request_id: text(&row, "request_id")?, request_hash: text(&row, "request_hash")?, identity, message_id: text(&row, "message_id")?, status, committed_event, expires_at: row.get_i64("record_expires_at") }));
        }
    }
    Ok(changes)
}

pub(super) async fn active_metadata(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
) -> TalosResult<Option<SyncActiveTurnMetadata>> {
    let rows = tx
        .query(
            "SELECT * FROM agent_active_turns WHERE session_id = ?",
            &[session_id],
        )
        .await?;
    if let Some(row) = rows.first() {
        let active = crate::session::agent_execution::record::parse_active_turn(row)?;
        if let Some(last_event) =
            super::session::key_at(adapter, tx, session_id, active.last_event_seq).await?
        {
            return Ok(Some(SyncActiveTurnMetadata {
                identity: active.identity,
                state: active.state,
                updated_at: active.updated_at,
                last_event,
            }));
        }
    }
    Ok(None)
}
