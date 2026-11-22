use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use talos_error::TalosResult;
use talos_sync_types::SyncChange as WireChange;

use crate::{
    codec_error, SyncActiveTurnMetadata, SyncChange, SyncResourceSnapshot, SyncSessionSnapshot,
};

/// Materialized Session facts used by the Talos Store for snapshot reads.
#[derive(Serialize, Deserialize)]
pub struct SessionProjection {
    pub snapshot: Option<SyncSessionSnapshot>,
    pub active_turns: Vec<SyncActiveTurnMetadata>,
}

impl SessionProjection {
    pub fn from_changes(changes: &[SyncChange]) -> Self {
        let terminal_turns: HashSet<_> = changes
            .iter()
            .filter_map(|change| {
                let SyncChange::Event(entry) = change else {
                    return None;
                };
                let event = entry.event.agent_event()?;
                event
                    .payload
                    .is_terminal()
                    .then_some(event.identity.turn_id.as_str())
            })
            .collect();
        let mut snapshot = None;
        let mut active_turns = Vec::new();
        for change in changes {
            match change {
                SyncChange::Metadata { after, .. } => {
                    if let SyncResourceSnapshot::Session(session) = after.as_ref() {
                        snapshot = Some(session.as_ref().clone());
                    }
                }
                SyncChange::ActiveTurnMetadata(active)
                    if !terminal_turns.contains(active.identity.turn_id.as_str()) =>
                {
                    active_turns.push(active.clone());
                }
                _ => {}
            }
        }
        Self {
            snapshot,
            active_turns,
        }
    }

    /// Classify a captured change before rebuilding the stored Session projection.
    pub fn needs_refresh(change: &WireChange) -> TalosResult<bool> {
        match change {
            WireChange::Fields { .. } | WireChange::Reset { .. } => Ok(true),
            WireChange::Record(record) => {
                match SyncChange::deserialize(&record.value).map_err(codec_error)? {
                    SyncChange::ActiveTurnMetadata(_) => Ok(true),
                    SyncChange::Event(entry) => Ok(entry
                        .event
                        .agent_event()
                        .is_some_and(|event| event.payload.is_terminal())),
                    _ => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }
}
