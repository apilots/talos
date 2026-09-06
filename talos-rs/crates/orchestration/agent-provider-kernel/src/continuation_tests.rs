use super::*;

#[test]
fn opaque_reference_round_trips_through_one_typed_private_schema() {
    let expected = KernelProviderContinuation::new(
        "action-1".to_string(),
        "call-1".to_string(),
        "search".to_string(),
        "message-1".to_string(),
        "block-1".to_string(),
        TurnExecutionMetrics::default(),
    );

    let decoded = KernelProviderContinuation::decode(&expected.encode().unwrap()).unwrap();

    assert_eq!(decoded.action_id, expected.action_id);
    assert_eq!(decoded.call_id, expected.call_id);
    assert_eq!(decoded.message_id, expected.message_id);
    assert_eq!(decoded.block_id, expected.block_id);
}
