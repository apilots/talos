use super::*;
use talos_agent_api::trajectory::*;

async fn fixture() -> (
    TalosService,
    Arc<DatabaseSessionStore>,
    Actor,
    AgentTurnRef,
    Arc<talos_store::execution::WorkerExecutionStore>,
) {
    let (service, store, journal) = test_application_with_worker(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::Cancelled,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
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
    crate::test_worker::seed_running_worker(
        service.sessions.as_ref(),
        store.as_ref(),
        journal.as_ref(),
        id.clone(),
        message,
    )
    .await;
    (service, store, actor, id, journal)
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
async fn finish(
    _store: &DatabaseSessionStore,
    id: &AgentTurnRef,
    journal: &talos_store::execution::WorkerExecutionStore,
) {
    let checkpoint = journal.checkpoint(id).await.unwrap().unwrap();
    let mut reducer =
        talos_agent::AgentEventReducer::from_checkpoint(id.clone(), checkpoint).unwrap();
    let event = CanonicalAgentEvent {
        identity: id.clone(),
        sequence: reducer.next_sequence(),
        occurred_at: 2,
        payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: talos_agent_api::AgentStopReason::EndTurn,
        }),
    };
    reducer.apply(event.clone()).unwrap();
    let scope = journal
        .session_scope(&id.session_id)
        .await
        .unwrap()
        .unwrap();
    journal
        .append(
            &scope,
            vec![event],
            &reducer.checkpoint(),
            Some(agent_protocol::AgentExecutionSettlement::Finished {
                metrics: reducer.checkpoint().metrics.clone(),
                memory_usage: vec![],
            }),
        )
        .await
        .unwrap();
}
#[tokio::test]
async fn tracing_reads_only_terminal_turns_and_preserves_old_null_and_original_access() {
    let (service, store, actor, id, journal) = fixture().await;
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
    finish(&store, &id, journal.as_ref()).await;
    service.reconcile_worker_events().await.unwrap();
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
    let (service, store, actor, id, journal) = fixture().await;
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
        metrics: None,
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
    journal
        .append_supplement(SessionHistorySupplement {
            identity: id.clone(),
            clock_segments: vec![],
            requests: vec![first, request(&id, 2)],
            tools: vec![],
            payloads: [("p".into(), payload.clone())].into(),
        })
        .await
        .unwrap();
    finish(&store, &id, journal.as_ref()).await;
    service.reconcile_worker_events().await.unwrap();
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
