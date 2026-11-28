use super::{
    metadata::{number, text},
    StoreSyncAdapter,
};
use talos_agent_api::{AgentEventPayload, TimelineEventPayload};
use talos_error::TalosResult;
use talos_storage::StorageTransaction;
use talos_sync::{
    SyncAcceptedCommand, SyncChange, SyncEventKey, SyncEventMapping, SyncExportFacts,
    SyncTimelineEntry, SyncTurnMetadata,
};

pub(super) async fn read_events(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    generation: u64,
    after: i64,
    through: i64,
) -> TalosResult<SyncExportFacts> {
    let mut facts = SyncExportFacts::default();
    let changes = &mut facts.changes;
    let mut cursor = after;
    loop {
        let rows = tx.query("SELECT event_seq, session_id, turn_id, sequence, occurred_at, persisted_at, payload_json FROM committed_agent_events WHERE session_id = ? AND history_generation = ? AND event_seq > ? AND event_seq <= ? ORDER BY event_seq LIMIT 128", &[session_id, &generation.to_string(), &cursor.to_string(), &through.to_string()]).await?;
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
                    super::session::key_at_generation(adapter, tx, session_id, generation, cursor)
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
                    let previous = tx.query("SELECT turn_id FROM committed_agent_events WHERE session_id = ? AND history_generation = ? AND event_seq < ? AND sequence = 1 ORDER BY event_seq DESC LIMIT 1", &[session_id, &generation.to_string(), &cursor.to_string()]).await?.first().and_then(|r| r.get_string("turn_id"));
                    changes.push(SyncChange::Turn(SyncTurnMetadata {
                        identity: event.identity.clone(),
                        coordinator_id: adapter.binding.peer_id.clone(),
                        previous_turn_id: previous,
                    }));
                }
            }
            facts.event_mappings.push(SyncEventMapping {
                scope: adapter.binding.scope.clone(),
                session_id: session_id.into(),
                generation,
                key: key.clone(),
                local_event_id: talos_types::TimelineEventId::from_seq(cursor).to_string(),
                local_event_seq: cursor,
            });
            changes.push(SyncChange::Event(SyncTimelineEntry {
                key,
                event: record.event,
                persisted_at: record.persisted_at,
            }));
        }
    }
    Ok(facts)
}

pub(super) async fn project_commands(
    adapter: &StoreSyncAdapter,
    tx: &mut dyn StorageTransaction,
    session_id: &str,
    generation: u64,
    rows: Vec<talos_storage::Row>,
) -> TalosResult<Vec<SyncChange>> {
    let mut changes = Vec::new();
    for row in rows {
        let identity = talos_types::AgentTurnRef::new(session_id, text(&row, "turn_id")?);
        if let Some(committed_event) = super::session::key_at_generation(
            adapter,
            tx,
            session_id,
            generation,
            number(&row, "committed_event_seq")?,
        )
        .await?
        {
            let command = match text(&row, "command")?.as_str() {
                "turn_submission" => talos_types::IdempotentCommand::TurnSubmission,
                "turn_input" => talos_types::IdempotentCommand::TurnInput,
                _ => {
                    return Err(talos_error::TalosError::internal(
                        "Invalid persisted command.",
                    ))
                }
            };
            let status = match text(&row, "outcome")?.as_str() {
                "accepted" => talos_types::TurnCommandStatus::Accepted,
                "failed" => talos_types::TurnCommandStatus::Failed,
                "failed_no_first_event" => talos_types::TurnCommandStatus::FailedNoFirstEvent,
                _ => {
                    return Err(talos_error::TalosError::internal(
                        "Invalid persisted command outcome.",
                    ))
                }
            };
            changes.push(SyncChange::AcceptedCommand(SyncAcceptedCommand {
                command,
                idempotency_key: text(&row, "idempotency_key")?,
                request_id: text(&row, "request_id")?,
                request_hash: text(&row, "request_hash")?,
                identity,
                message_id: text(&row, "message_id")?,
                status,
                committed_event,
                expires_at: row.get_i64("record_expires_at"),
            }));
        }
    }
    Ok(changes)
}
