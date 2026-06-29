use super::*;

#[test]
fn canonical_method_matches_talos_timeline_event_type() {
    assert_eq!(
        canonical_method(&TalosTimelineEventType::ItemDelta),
        "timeline.item.delta"
    );
}
