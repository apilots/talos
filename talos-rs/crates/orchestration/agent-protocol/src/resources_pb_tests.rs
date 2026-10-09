use talos_agent_api::{
    AgentRunMetrics, AgentSystemNotice, AgentTurnCompleted, SystemNoticeContent,
};
use talos_types::{AgentTurnRef, TalosTimelineNoticeLevel};

use super::*;

#[test]
fn completed_history_and_product_notice_survive_the_rpc_projection() {
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let value = SessionHistoryTurn {
        identity: identity.clone(),
        result: AgentRunResult::Completed(AgentRunOutput {
            messages: Vec::new(),
            metrics: AgentRunMetrics::default(),
            completion: AgentTurnCompleted {
                stop_reason: AgentStopReason::TransferUnresolved,
            },
        }),
        notices: vec![TimelineEvent {
            event_id: "event-7".into(),
            event_seq: 7,
            persisted_at: 100,
            event: TimelineEventPayload::SystemNotice(TimelineSystemNotice {
                identity,
                occurred_at: 99,
                notice: AgentSystemNotice {
                    level: TalosTimelineNoticeLevel::Warning,
                    content: SystemNoticeContent::Text("context trimmed".into()),
                },
            }),
        }],
        completed_event_seq: 8,
        completed_at: 101,
    };
    let wire = encode_history_turn(value.clone()).unwrap();
    assert_eq!(decode_history_turn(wire).unwrap(), value);
}
