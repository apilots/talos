use super::*;
use talos_agent_api::trajectory::*;
use talos_store::session::{AgentExecutionStore, CommitTerminalSettlementParams};

async fn fixture() -> (TalosService, Arc<DatabaseSessionStore>, Actor, AgentTurnRef) {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("trajectory-session", request_source(), None),
        )
        .await
        .unwrap();
    let id = AgentTurnRef::new("trajectory-session", "turn");
    let message = crate::turn::completed_agent_user_input(
        &id.turn_id,
        "input",
        crate::test_support::test_user_input("hello"),
        1,
    )
    .unwrap();
    crate::test_support::commit_test_running_turn(store.as_ref(), id.clone(), message).await;
    (service, store, actor, id)
}
fn request(id: &AgentTurnRef, n: u32) -> TrajectoryReadModelRequestResponse {
    TrajectoryReadModelRequestResponse {
        identity: id.clone(),
        request: ModelRequestTrace {
            model_request_id: format!("r{n}"),
            ordinal: n,
            purpose: ModelRequestPurpose::Working,
            connection_id: "main".into(),
            model_id: "model".into(),
            call_records: vec![],
        },
        prompt: None,
        tools: None,
        parameters: None,
        outputs: vec![],
    }
}
async fn finish(store: &DatabaseSessionStore, id: &AgentTurnRef) {
    let active = store
        .get_agent_active_turn(&id.session_id)
        .await
        .unwrap()
        .unwrap();
    let last = store
        .list_agent_turn_events(id, 100)
        .await
        .unwrap()
        .last()
        .unwrap()
        .sequence;
    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: talos_agent_api::AgentEvent {
                identity: id.clone(),
                sequence: last + 1,
                occurred_at: 2,
                payload: talos_agent_api::AgentEventPayload::TurnCompleted(
                    talos_agent_api::AgentTurnCompleted {
                        stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                    },
                ),
            },
            expected_event_seq: active.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn tracing_reads_only_terminal_turns_and_preserves_old_null_and_original_access() {
    let (service, store, actor, id) = fixture().await;
    let params = TrajectoryReadTurnRequest {
        identity: id.clone(),
        cursor: None,
        limit: None,
    };
    assert!(service
        .read_turn_trajectory(&actor, params.clone())
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ResourceNotFound));
    finish(&store, &id).await;
    let value = service
        .read_turn_trajectory(&actor, params.clone())
        .await
        .unwrap();
    assert_eq!(value.data, None);
    assert!(value.clock_segments.is_empty());
    let stranger = Actor::regular("stranger");
    assert!(service
        .read_turn_trajectory(&stranger, params)
        .await
        .is_err());
    assert!(service
        .read_trajectory_payload(
            &stranger,
            TrajectoryReadPayloadRequest {
                identity: id,
                payload_id: "copied".into(),
                cursor: None,
                limit_bytes: None
            }
        )
        .await
        .is_err());
}
#[tokio::test]
async fn terminal_pages_preserve_utf8_raw_arguments_scope_and_immutable_position() {
    let (service, store, actor, id) = fixture().await;
    let raw = "{ \"text\": \"你好🙂\" }\n";
    let payload = TrajectoryPayload::Response {
        parts: vec![ResponsePart::ToolCall {
            call: SnapshotToolCall {
                call_id: "call".into(),
                name: "Read".into(),
                arguments: serde_json::json!({"text":"你好🙂"}).into(),
                arguments_text: Some(raw.into()),
            },
        }],
    };
    let mut first = request(&id, 1);
    first.request.call_records = vec![ModelCallRecord {
        call_record_id: "attempt".into(),
        ordinal: 1,
        observed_state: ModelCallState::Completed,
        timing: None,
        usage: None,
        finish_reason: None,
        error: None,
    }];
    first.outputs = vec![ModelCallOutput {
        call_record_id: "attempt".into(),
        response: Some(PayloadRef {
            payload_id: "p".into(),
            complete: Some(true),
        }),
    }];
    store
        .append_history_supplement(SessionHistorySupplement {
            identity: id.clone(),
            clock_segments: vec![],
            requests: vec![first, request(&id, 2)],
            tools: vec![],
            payloads: [("p".into(), payload.clone())].into(),
        })
        .await
        .unwrap();
    finish(&store, &id).await;
    let mut cursor = None;
    let mut assembled = String::new();
    loop {
        let page = service
            .read_trajectory_payload(
                &actor,
                TrajectoryReadPayloadRequest {
                    identity: id.clone(),
                    payload_id: "p".into(),
                    cursor,
                    limit_bytes: Some(7),
                },
            )
            .await
            .unwrap();
        assert!(page.data.len() <= 7);
        assembled.push_str(&page.data);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        serde_json::from_str::<TrajectoryPayload>(&assembled).unwrap(),
        payload
    );
    let page = service
        .read_turn_trajectory(
            &actor,
            TrajectoryReadTurnRequest {
                identity: id.clone(),
                cursor: None,
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    let second = service
        .read_turn_trajectory(
            &actor,
            TrajectoryReadTurnRequest {
                identity: id.clone(),
                cursor: page.next_cursor,
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert!(
        matches!(&second.data.unwrap()[0],TrajectoryEntry::ModelRequest{request} if request.ordinal==2)
    );
    assert!(service
        .read_trajectory_payload(
            &actor,
            TrajectoryReadPayloadRequest {
                identity: AgentTurnRef::new(&id.session_id, "other"),
                payload_id: "p".into(),
                cursor: None,
                limit_bytes: None
            }
        )
        .await
        .is_err());
    store.delete_session_record(&id.session_id).await.unwrap();
    assert!(service
        .read_trajectory_payload(
            &actor,
            TrajectoryReadPayloadRequest {
                identity: id,
                payload_id: "p".into(),
                cursor: None,
                limit_bytes: None
            }
        )
        .await
        .is_err());
}
