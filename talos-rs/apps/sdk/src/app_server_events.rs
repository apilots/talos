use talos_types::talos::{TalosTimelineEventType, TimelineEventEnvelope};

pub(crate) fn is_terminal_application_event(event: &TimelineEventEnvelope) -> bool {
    matches!(
        event.event.event_type,
        TalosTimelineEventType::TurnCompleted | TalosTimelineEventType::TurnError
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_types::talos::{
        StopReason, TalosTimelineEvent, TalosTimelineEventPayload, TalosTurnCompletedEvent,
        TalosUsage,
    };

    #[test]
    fn detects_terminal_turn_events() {
        let terminal = TimelineEventEnvelope {
            event_id: "event-1".to_string(),
            event_seq: 1,
            created_at_ms: 1,
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            message_id: "message-1".to_string(),
            event: TalosTimelineEvent {
                event_seq: Some(1),
                event_id: Some("event-1".to_string()),
                created_at_ms: Some(1),
                event_type: TalosTimelineEventType::TurnCompleted,
                turn_id: "turn-1".to_string(),
                message_id: "message-1".to_string(),
                session_id: "session-1".to_string(),
                sequence: 7,
                payload: TalosTimelineEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                    request_id: "request-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    completed_at_ms: 1,
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    metrics: None,
                }),
            },
        };

        assert!(is_terminal_application_event(&terminal));
    }
}
