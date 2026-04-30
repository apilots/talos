use talos_app_protocol::events::ApplicationEventEnvelope;
use talos_error::TalosResult;
use talos_types::talos::{TalosEvent, TalosEventType};

pub(crate) fn is_terminal_application_event(event: &ApplicationEventEnvelope) -> bool {
    matches!(
        event.event.event_type,
        TalosEventType::MessageCompleted | TalosEventType::MessageError
    )
}

pub(crate) fn map_application_event(envelope: ApplicationEventEnvelope) -> TalosResult<TalosEvent> {
    Ok(envelope.event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_types::talos::{
        TalosEventPayload, TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent, TalosUsage,
    };

    fn envelope(event: TalosEvent) -> ApplicationEventEnvelope {
        ApplicationEventEnvelope {
            subscription_id: "sub-1".to_string(),
            event_id: "event-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            event,
        }
    }

    #[test]
    fn maps_protocol_event_by_reusing_server_talos_event() {
        let source = TalosEvent {
            event_type: TalosEventType::MessageOutputDelta,
            message_id: "message-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 7,
            payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                request_id: "request-1".to_string(),
                delta: "hello".to_string(),
                item_index: 0,
                iteration: 0,
                step_id: "step-1".to_string(),
            }),
        };

        let event = map_application_event(envelope(source)).expect("event should map");

        assert_eq!(event.event_type, TalosEventType::MessageOutputDelta);
        assert_eq!(event.message_id, "message-1");
        assert_eq!(event.sequence, 7);
    }

    #[test]
    fn detects_terminal_turn_events() {
        let terminal = envelope(TalosEvent {
            event_type: TalosEventType::MessageCompleted,
            message_id: "message-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 8,
            payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                request_id: "request-1".to_string(),
                stop_reason: Default::default(),
                usage: TalosUsage::default(),
                total_output_items: 1,
                iteration: 0,
                step_id: "step-1".to_string(),
                task_count: 0,
            }),
        });

        assert!(is_terminal_application_event(&terminal));
    }
}
