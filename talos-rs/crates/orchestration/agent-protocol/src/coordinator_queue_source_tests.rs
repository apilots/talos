use super::*;

#[test]
fn coordinator_queue_round_trip_preserves_trusted_loop_source() {
    let queue = MessageQueue {
        revision: 1,
        max_messages: 10,
        messages: vec![
            QueuedMessage {
                id: "human-1".into(),
                input: AgentUserInput {
                    content: vec![AgentUserContent::Text {
                        text: "Help".into(),
                    }],
                },
                source: QueuedMessageSource::User,
            },
            QueuedMessage {
                id: "loop-occurrence-1".into(),
                input: AgentUserInput {
                    content: vec![AgentUserContent::Text {
                        text: "Check CI".into(),
                    }],
                },
                source: QueuedMessageSource::Loop {
                    loop_id: "loop-1".into(),
                    occurrence_id: "occurrence-1".into(),
                },
            },
        ],
    };
    let wire = encode_reply(ActiveSessionReply::Queue(queue.clone()));
    let ActiveSessionReply::Queue(decoded) = decode_reply(wire).unwrap() else {
        panic!("queue response expected")
    };
    assert_eq!(decoded, queue);
}

#[test]
fn coordinator_queue_rejects_incomplete_loop_identity() {
    let reply = pb::ActiveSessionCommandResponse {
        result: Some(pb::active_session_command_response::Result::Queue(
            pb::CoordinatorMessageQueue {
                revision: 1,
                max_messages: 10,
                messages: vec![pb::CoordinatorQueueMessage {
                    input: Some(input(AgentUserInput {
                        content: vec![AgentUserContent::Text {
                            text: "Check CI".into(),
                        }],
                    })),
                    message_id: "occurrence".into(),
                    source: Some(pb::coordinator_queue_message::Source::Loop(
                        pb::CoordinatorQueueLoopSource {
                            loop_id: "".into(),
                            occurrence_id: "".into(),
                        },
                    )),
                }],
            },
        )),
    };
    assert!(decode_reply(reply).is_err());
}
