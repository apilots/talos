use talos_app_protocol::events::ApplicationEventEnvelope;
use talos_session_store::SessionEventRecord;
use talos_types::talos::TalosEvent;

pub fn notification_envelope(
    subscription_id: &str,
    record: SessionEventRecord,
) -> ApplicationEventEnvelope {
    let event = TalosEvent {
        event_type: record.event_type,
        message_id: record.message_id,
        session_id: record.session_id,
        sequence: record.sequence,
        payload: record.payload,
    };
    ApplicationEventEnvelope {
        subscription_id: subscription_id.to_string(),
        event_id: record.event_id.to_string(),
        turn_id: record.turn_id,
        event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_session_store::SessionEventId;
    use talos_types::talos::{TalosEventPayload, TalosEventType, TalosMessageOutputDeltaEvent};

    #[test]
    fn record_maps_to_talos_event_notification_without_payload_translation() {
        let record = SessionEventRecord {
            event_seq: 1,
            event_id: SessionEventId::from_seq(1),
            session_id: "s1".to_string(),
            turn_id: Some("t1".to_string()),
            message_id: "m1".to_string(),
            sequence: 1,
            event_type: TalosEventType::MessageOutputDelta,
            payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                request_id: "r1".to_string(),
                delta: "hello".to_string(),
                item_index: 0,
                iteration: 0,
                step_id: "step".to_string(),
            }),
            created_at: 1,
        };

        let envelope = notification_envelope("sub1", record);

        assert_eq!(
            envelope.event.event_type,
            TalosEventType::MessageOutputDelta
        );
        assert_eq!(envelope.event.message_id, "m1");
        assert_eq!(envelope.event.sequence, 1);
    }
}
