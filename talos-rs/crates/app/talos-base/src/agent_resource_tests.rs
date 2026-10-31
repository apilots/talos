//! Regression for protected local callbacks and foreign leases at the current owner.
use super::*;
use agent_protocol::{harness_pb, NodeAuth};
use pb::agent_session_data_rpc_server::AgentSessionDataRpc;

#[tokio::test]
async fn local_callback_ports_and_current_invocation_use_the_persisted_session_owner() {
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
    let nodes = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(
        storage.pool().clone(),
    )));
    let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let record = session_record();
    sessions.insert_session_record(&record).await.unwrap();
    let data = NodeSessionDataRpc::new(
        nodes.clone(),
        sessions,
        Arc::new(EmptyHistory),
        Arc::new(TestAttachments),
        Arc::new(TestMemoryProgress::default()),
    );
    let auth = NodeAuth {
        node_id: record.execution_node_id.clone(),
        token: "private-host-token".into(),
    };
    let (host, resources) = crate::local_resource_rpc::LocalResourceRpc::start(
        auth.clone(),
        data.clone(),
        NodeToolImageRpc::new(nodes, Arc::new(TestToolImages), 1024),
        Arc::new(TestAgentControl),
    )
    .await
    .unwrap();
    let context = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id.clone(),
        session_id: record.id.clone(),
        workspace: talos_config::Config::default()
            .resolve_workspace("default", "user-1")
            .unwrap(),
    };
    let resources = resources
        .for_invocation(auth.clone(), &context, None)
        .unwrap();
    assert!(SessionHistoryReader::read(
        &resources,
        SessionHistoryReadParams {
            session_id: record.id.clone(),
            window: talos_agent::SessionHistoryWindow::Latest,
            limit: 10,
        }
    )
    .await
    .unwrap()
    .is_empty());
    resources
        .advance_processed_until_event_seq(&record.id, 0, 7)
        .await
        .unwrap();
    assert_eq!(
        resources
            .processed_until_event_seq(&record.id)
            .await
            .unwrap(),
        7
    );
    let caller = talos_types::AgentCaller {
        session_id: record.id.clone(),
        turn_id: "turn-1".into(),
        agent_id: record.agent_id.clone(),
    };
    assert!(AgentControl::list(
        &resources,
        ListSubagentsRequest {
            caller: caller.clone()
        }
    )
    .await
    .unwrap()
    .children
    .is_empty());
    let images = ToolImageStore::save(
        &resources,
        "ignored-by-wire",
        vec![Attachment::new(
            AttachmentKind::Image,
            "image.png",
            "image/png",
            bytes::Bytes::from_static(b"image"),
        )],
    )
    .await
    .unwrap();
    assert_eq!(images[0].file_id, "image-1");
    let scope = pb::CoordinatorCallContext {
        coordinator_id: String::new(),
        connection_id: uuid::Uuid::new_v4().to_string(),
        rpc_call_id: 1,
        session: Some(harness_pb::encode_session_context(context)),
        turn_id: Some(caller.turn_id),
        project_id: None,
    };
    // The callback owner has the original Session facts but no foreign Worker lease row.
    let owner = data.for_invocation(record.execution_node_id, scope.clone());
    let request = |session_id| {
        Request::new(pb::ReadMemoryProgressRequest {
            auth: Some(pb::NodeAuth {
                node_id: auth.node_id.clone(),
                token: "foreign-lease".into(),
            }),
            caller: Some(scope.clone()),
            params: Some(pb::ReadMemoryProgressParams { session_id }),
        })
    };
    let result = owner
        .read_memory_progress(request(record.id))
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
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn history_supplement_stream_round_trips_large_invocation_and_uses_original_history() {
    use talos_agent_api::{trajectory::*, AgentTurnRef};
    use talos_storage::Storage;
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
    let nodes = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(
        storage.pool().clone(),
    )));
    let sessions = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let record = session_record();
    sessions.insert_session_record(&record).await.unwrap();
    let event = serde_json::to_string(&talos_agent_api::AgentEventPayload::TurnStarted(
        talos_agent_api::AgentTurnStarted {},
    ))
    .unwrap();
    storage.execute("INSERT INTO agent_events(session_id,turn_id,sequence,occurred_at,persisted_at,payload_json) VALUES (?, 't',1,1,1,?)", &[&record.id,&event]).await.unwrap();
    let history = Arc::new(talos_service::SessionHistoryService::new(Arc::new(
        talos_service::TimelineEventService::new(sessions.clone()),
    )));
    let data = NodeSessionDataRpc::new(
        nodes.clone(),
        sessions.clone(),
        history,
        Arc::new(TestAttachments),
        Arc::new(TestMemoryProgress::default()),
    );
    let auth = NodeAuth {
        node_id: record.execution_node_id.clone(),
        token: "private-host-token".into(),
    };
    let (host, resources) = crate::local_resource_rpc::LocalResourceRpc::start(
        auth.clone(),
        data,
        NodeToolImageRpc::new(nodes, Arc::new(TestToolImages), 1024),
        Arc::new(TestAgentControl),
    )
    .await
    .unwrap();
    let context = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id.clone(),
        session_id: record.id.clone(),
        workspace: talos_config::Config::default()
            .resolve_workspace("default", "user-1")
            .unwrap(),
    };
    let resources = resources.for_invocation(auth, &context, None).unwrap();
    let identity = AgentTurnRef::new(&record.id, "t");
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
    SessionHistoryReader::append_supplement(&resources, supplement.clone())
        .await
        .unwrap();
    let row = storage.query("SELECT payload_json, execution_capture_json FROM agent_events WHERE session_id=? AND turn_id='t' AND sequence=1", &[&record.id]).await.unwrap();
    assert_eq!(row[0].get_string("payload_json").unwrap(), event);
    let saved: SessionHistorySupplement =
        serde_json::from_str(&row[0].get_string("execution_capture_json").unwrap()).unwrap();
    assert_eq!(saved, supplement);
    sessions.delete_session_record(&record.id).await.unwrap();
    assert!(
        SessionHistoryReader::append_supplement(&resources, supplement)
            .await
            .is_err()
    );
    host.shutdown().await.unwrap();
}
