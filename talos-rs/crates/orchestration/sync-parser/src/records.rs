use serde::Serialize;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_types::{SyncChange as WireChange, SyncRecord, SyncStream};
use talos_types::AgentTurnRef;

use crate::{codec_error, SyncChange, SyncEventKey, SyncResourceKey, SyncTurnMetadata};

pub(super) fn encode(resource: &SyncResourceKey, change: &SyncChange) -> TalosResult<WireChange> {
    match change {
        SyncChange::Metadata { .. } => Err(invalid_record()),
        SyncChange::Reset { history_generation } => {
            session(resource, resource.id())?;
            Ok(WireChange::Reset {
                history_generation: *history_generation,
            })
        }
        SyncChange::Delete => Ok(WireChange::Delete),
        SyncChange::Turn(turn) => {
            session(resource, &turn.identity.session_id)?;
            Ok(WireChange::Stream(SyncStream {
                id: turn.identity.turn_id.clone(),
                owner: turn.coordinator_id.clone(),
                previous: turn.previous_turn_id.clone(),
            }))
        }
        change => record(resource, change).map(WireChange::Record),
    }
}

fn key(kind: &str, value: &impl Serialize) -> TalosResult<String> {
    serde_json::to_string(&(kind, value)).map_err(codec_error)
}

fn record(resource: &SyncResourceKey, change: &SyncChange) -> TalosResult<SyncRecord> {
    let (identity, identity_key, publisher, sequence, reference, replaceable) = match change {
        SyncChange::Event(entry) => {
            let identity = entry.event.identity();
            let (publisher, sequence) = match (&entry.key, &entry.event) {
                (
                    SyncEventKey::Agent {
                        identity: expected,
                        sequence,
                    },
                    talos_agent_api::TimelineEventPayload::Agent(event),
                ) if expected == identity && sequence == &event.sequence => (None, Some(*sequence)),
                (
                    SyncEventKey::Notice {
                        peer_id,
                        source_event_id,
                    },
                    talos_agent_api::TimelineEventPayload::SystemNotice(_),
                ) if !source_event_id.is_empty() => (Some(peer_id.clone()), None),
                _ => return Err(invalid_record()),
            };
            (
                identity,
                key("event", &entry.key)?,
                publisher,
                sequence,
                None,
                false,
            )
        }
        SyncChange::ActiveTurnMetadata(active) => (
            &active.identity,
            key("active", &active.identity.turn_id)?,
            None,
            None,
            Some(key("event", &active.last_event)?),
            true,
        ),
        SyncChange::AcceptedCommand(command) => {
            if command.idempotency_key.is_empty() || command.request_id.is_empty() {
                return Err(invalid_record());
            }
            (
                &command.identity,
                key(
                    "command",
                    &(
                        &command.command,
                        &command.idempotency_key,
                        &command.request_id,
                    ),
                )?,
                None,
                None,
                Some(key("event", &command.committed_event)?),
                false,
            )
        }
        _ => return Err(invalid_record()),
    };
    session(resource, &identity.session_id)?;
    Ok(SyncRecord {
        key: identity_key,
        stream: identity.turn_id.clone(),
        publisher,
        sequence,
        reference,
        replaceable,
        value: serde_json::to_value(change).map_err(codec_error)?,
    })
}

pub(super) fn decode(resource: &SyncResourceKey, change: WireChange) -> TalosResult<SyncChange> {
    match change {
        WireChange::Fields { .. } => Err(invalid_record()),
        WireChange::Delete => Ok(SyncChange::Delete),
        WireChange::Reset { history_generation } => {
            session(resource, resource.id())?;
            Ok(SyncChange::Reset { history_generation })
        }
        WireChange::Stream(stream) => {
            session(resource, resource.id())?;
            Ok(SyncChange::Turn(SyncTurnMetadata {
                identity: AgentTurnRef::new(resource.id(), stream.id),
                coordinator_id: stream.owner,
                previous_turn_id: stream.previous,
            }))
        }
        WireChange::Record(wire) => {
            let change = serde_json::from_value(wire.value.clone()).map_err(codec_error)?;
            let expected = record(resource, &change)?;
            if wire != expected {
                return Err(invalid_record());
            }
            Ok(change)
        }
    }
}

fn session(resource: &SyncResourceKey, id: &str) -> TalosResult<()> {
    if matches!(resource, SyncResourceKey::Session { session_id } if session_id == id) {
        Ok(())
    } else {
        Err(invalid_record())
    }
}

fn invalid_record() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The synchronized Talos record does not match its identity.",
    )
}
