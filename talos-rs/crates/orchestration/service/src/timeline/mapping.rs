use talos_agent_api::{AgentEvent, TimelineActiveTurn, TimelineEvent};
use talos_store::session::{AgentActiveTurnRecord, AgentEventRecord};

use super::TimelineEventId;

pub(crate) fn map_timeline_event(record: AgentEventRecord) -> TimelineEvent {
    TimelineEvent {
        event_id: TimelineEventId::from_seq(record.event_seq).to_string(),
        event_seq: record.event_seq,
        persisted_at: record.persisted_at,
        event: AgentEvent {
            identity: record.identity,
            sequence: record.sequence,
            occurred_at: record.occurred_at,
            payload: record.payload,
        },
    }
}

pub(crate) fn map_timeline_active_turn(record: AgentActiveTurnRecord) -> TimelineActiveTurn {
    TimelineActiveTurn {
        identity: record.identity,
        initial_message_id: record.initial_message_id,
        state: record.state,
        last_event_id: TimelineEventId::from_seq(record.last_event_seq).to_string(),
        updated_at: record.updated_at,
    }
}
