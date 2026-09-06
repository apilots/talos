use super::*;

#[test]
fn terminal_event_round_trips_with_one_payload_discriminator() {
    let expected = AgentEvent {
        identity: AgentTurnRef::new("session-1", "turn-1"),
        sequence: 7,
        occurred_at: 100,
        payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    };

    let encoded = serde_json::to_value(&expected).expect("serialize Agent event");
    let decoded: AgentEvent = serde_json::from_value(encoded.clone()).expect("decode Agent event");

    assert_eq!(decoded, expected);
    assert_eq!(encoded["payload"]["type"], "turnCompleted");
    assert!(encoded.get("eventType").is_none());
}

#[test]
fn pending_input_variant_fields_use_the_protocol_casing() {
    let pending = AgentPendingInput::Message {
        action_id: "action-1".to_string(),
        prompt: "Continue".to_string(),
    };

    let encoded = serde_json::to_value(&pending).expect("serialize pending input");

    assert_eq!(encoded["actionId"], "action-1");
    assert!(encoded.get("action_id").is_none());
}
