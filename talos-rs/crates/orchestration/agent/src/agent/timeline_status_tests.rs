use super::*;
use talos_agent_kernel::{
    KernelStreamEvent, KernelStreamEventData, KernelStreamEventType, ToolExecutingStreamData,
    ToolQueuedStreamData, ToolStreamKey,
};
use talos_types::talos::{AgentEventPayload, TalosTimelineDelta, TalosTimelineItemStatus};

#[test]
fn test_tool_queued_and_executing_update_timeline_status() {
    let mut timeline = TimelineBuilder::new(
        "request-tool-status",
        "turn-tool-status",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();
    let stream_key = ToolStreamKey { round: 0, index: 0 };

    let queued_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ToolQueued,
            KernelStreamEventData::ToolQueued(ToolQueuedStreamData {
                stream_key: stream_key.clone(),
                call_id: "call-read".to_string(),
                tool_name: "Read".to_string(),
            }),
        ),
    )
    .expect("tool queued should be mapped");
    let item_id = match &queued_events[0].payload {
        AgentEventPayload::ItemStarted(payload) => payload.item.item_id.clone(),
        _ => panic!("expected tool item start"),
    };
    assert_eq!(
        timeline
            .item(&item_id)
            .expect("tool item should exist")
            .status,
        TalosTimelineItemStatus::Queued
    );
    assert!(queued_events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemDelta(payload)
            if payload.item_id == item_id
                && matches!(
                    payload.delta,
                    TalosTimelineDelta::Status {
                        status: TalosTimelineItemStatus::Queued
                    }
                )
    )));

    let executing_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ToolExecuting,
            KernelStreamEventData::ToolExecuting(ToolExecutingStreamData {
                stream_key,
                call_id: "call-read".to_string(),
                tool_name: "Read".to_string(),
            }),
        ),
    )
    .expect("tool executing should be mapped");
    assert_eq!(
        timeline
            .item(&item_id)
            .expect("tool item should exist")
            .status,
        TalosTimelineItemStatus::Running
    );
    assert!(executing_events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemDelta(payload)
            if payload.item_id == item_id
                && matches!(
                    payload.delta,
                    TalosTimelineDelta::Status {
                        status: TalosTimelineItemStatus::Running
                    }
                )
    )));
}
