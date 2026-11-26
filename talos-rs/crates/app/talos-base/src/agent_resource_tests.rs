//! Resource authorization and history regressions at the original owner.
use super::*;
use agent_protocol::harness_pb;
use pb::agent_session_data_rpc_server::AgentSessionDataRpc;

#[tokio::test]
async fn reverse_invocation_is_bound_to_the_persisted_session() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let nodes = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(&storage)));
    let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let record = session_record();
    sessions.insert_session_record(&record).await.unwrap();
    let memory = Arc::new(TestMemoryProgress::default());
    memory
        .advance_processed_until_event_seq(&record.id, 0, 7)
        .await
        .unwrap();
    let data = NodeSessionDataRpc::new(
        nodes,
        sessions,
        Arc::new(EmptyHistory),
        Arc::new(TestAttachments),
        memory,
    );
    let context = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id.clone(),
        session_id: record.id.clone(),
        workspace: talos_config::Config::default()
            .resolve_workspace("default", "user-1")
            .unwrap(),
    };
    let scope = pb::CoordinatorCallContext {
        coordinator_id: String::new(),
        connection_id: uuid::Uuid::new_v4().to_string(),
        rpc_call_id: 1,
        session: Some(harness_pb::encode_session_context(context)),
        turn_id: Some("turn-1".into()),
        project_id: None,
    };
    // The callback owner has the original Session facts but no foreign Worker lease row.
    let owner = data.for_invocation(record.execution_node_id.clone(), scope.clone());
    let request = |session_id| {
        Request::new(pb::ReadMemoryProgressRequest {
            auth: Some(pb::NodeAuth {
                node_id: record.execution_node_id.clone(),
                token: "foreign-lease".into(),
            }),
            caller: Some(scope.clone()),
            params: Some(pb::ReadMemoryProgressParams { session_id }),
        })
    };
    let result = owner
        .read_memory_progress(request(record.id.clone()))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        result.result,
        Some(pb::read_memory_progress_response::Result::Progress(
            pb::MemoryProgress {
                processed_until_event_seq: 7
            }
        ))
    ));
    let result = owner
        .read_memory_progress(request("unrelated-session".into()))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        result.result,
        Some(pb::read_memory_progress_response::Result::Failure(_))
    ));
}

pub(super) async fn assert_large_history_supplement_round_trip(
    storage: &SqliteStorage,
    sessions: &DatabaseSessionStore,
    resources: &AgentSessionDataClient,
    session_id: &str,
) {
    use talos_agent_api::{trajectory::*, AgentTurnRef};
    use talos_storage::Storage;

    let event = serde_json::to_string(&talos_agent_api::AgentEventPayload::TurnStarted(
        talos_agent_api::AgentTurnStarted {},
    ))
    .unwrap();
    storage
        .execute(
            "INSERT INTO agent_events(session_id, turn_id, sequence, occurred_at, persisted_at, payload_json)
             VALUES (?, 't', 1, 1, 1, ?)",
            &[session_id, &event],
        )
        .await
        .unwrap();
    let identity = AgentTurnRef::new(session_id, "t");
    let mut supplement = SessionHistorySupplement {
        identity: identity.clone(),
        clock_segments: vec![],
        requests: vec![],
        tools: vec![],
        payloads: Default::default(),
    };
    for n in 0..2 {
        let payload_id = format!("body-{n}");
        let call_record_id = format!("attempt-{n}");
        supplement.payloads.insert(
            payload_id.clone(),
            TrajectoryPayload::Response {
                parts: vec![ResponsePart::Text {
                    text: format!("{n}你好").repeat(400000),
                }],
            },
        );
        supplement
            .requests
            .push(TrajectoryReadModelRequestResponse {
                identity: identity.clone(),
                request: ModelRequestTrace {
                    model_request_id: format!("request-{n}"),
                    ordinal: n + 1,
                    purpose: ModelRequestPurpose::Working,
                    connection_id: "configured".into(),
                    model_id: "fixture".into(),
                    call_records: vec![ModelCallRecord {
                        call_record_id: call_record_id.clone(),
                        ordinal: 1,
                        observed_state: ModelCallState::Completed,
                        metrics: None,
                        finish_reason: Some("stop".into()),
                        error: None,
                    }],
                },
                prompt: None,
                tools: None,
                parameters: None,
                outputs: vec![ModelCallOutput {
                    call_record_id,
                    response: Some(PayloadRef {
                        payload_id,
                        complete: Some(true),
                    }),
                }],
            });
    }
    assert!(serde_json::to_vec(&supplement).unwrap().len() > 4 * 1024 * 1024);
    SessionHistoryReader::append_supplement(resources, supplement.clone())
        .await
        .unwrap();
    let row = storage
        .query(
            "SELECT payload_json, execution_capture_json FROM agent_events
             WHERE session_id = ? AND turn_id = 't' AND sequence = 1",
            &[session_id],
        )
        .await
        .unwrap();
    assert_eq!(row[0].get_string("payload_json").unwrap(), event);
    let saved: SessionHistorySupplement =
        serde_json::from_str(&row[0].get_string("execution_capture_json").unwrap()).unwrap();
    assert_eq!(saved, supplement);
    sessions.delete_session_record(session_id).await.unwrap();
    assert!(
        SessionHistoryReader::append_supplement(resources, supplement)
            .await
            .is_err()
    );
}
