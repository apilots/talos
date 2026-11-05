use talos_agent_api::{
    AgentRunMetrics, AgentSystemNotice, AgentTurnCompleted, SystemNoticeContent,
};
use talos_types::{AgentTurnRef, TalosTimelineNoticeLevel};

use super::*;

#[test]
fn completed_history_and_product_notice_survive_the_rpc_projection() {
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let value = SessionHistoryTurn {
        automation_input: None,
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

#[test]
fn exact_automation_input_survives_worker_history_rpc() {
    for origin in [
        talos_types::AutomationTurnOrigin::Goal {
            goal_id: "goal".into(),
            objective_revision: 2,
        },
        talos_types::AutomationTurnOrigin::Loop {
            loop_id: "loop".into(),
            occurrence_id: "occurrence".into(),
            config_revision: 3,
        },
    ] {
        let value = SessionHistoryTurn {
            automation_input: Some(talos_types::AutomationInputOrigin {
                message_id: "initial-input".into(),
                origin,
            }),
            identity: AgentTurnRef::new("s", "t"),
            result: AgentRunResult::Completed(AgentRunOutput {
                messages: vec![],
                metrics: Default::default(),
                completion: AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                },
            }),
            notices: vec![],
            completed_event_seq: 4,
            completed_at: 10,
        };
        let wire = encode_history_turn(value.clone()).unwrap();
        assert_eq!(decode_history_turn(wire).unwrap(), value);
    }
}
#[test]
fn malformed_optional_automation_input_is_rejected_instead_of_dropped() {
    for value in [
        pb::AutomationInputOrigin {
            message_id: String::new(),
            origin: None,
        },
        pb::AutomationInputOrigin {
            message_id: "input".into(),
            origin: None,
        },
    ] {
        assert!(crate::automation_pb::decode_input_origin(value).is_err());
    }
}
