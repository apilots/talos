use super::*;

#[test]
fn canonical_method_matches_agent_event_type() {
    assert_eq!(
        canonical_method(&AgentEventType::ItemDelta),
        "timeline.item.delta"
    );
}
