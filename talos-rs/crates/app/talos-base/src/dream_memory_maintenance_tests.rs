use super::*;
use async_trait::async_trait;
use opendal::{services::Memory, Operator};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageBlock, AgentMessageBlockCompleted,
    AgentMessageBlockContent, AgentMessageBlockStarted, AgentMessageBlockStatus, AgentMessageRole,
    AgentMessageStarted, AgentMessageStatus, AgentStopReason, AgentTurnCompleted, AgentTurnRef,
    AgentTurnStarted,
};
use talos_auth::{AuthService, OpenModeUserService, RequestIdentity};
use talos_coordinator::CoordinatorImpl;
use talos_memory::{
    DreamMemoryDate, DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryProvenance,
    MemoryAgentId, MemoryTimezone, MemoryUserId,
};
use talos_storage::{SqliteStorage, Storage};
use talos_store::file::DatabaseFileStore;
use talos_store::profile::{MockProfileStore, ProfileUsageUpdate};
use talos_store::project::StorageProjectStore;
use talos_store::session::{AgentExecutionStore, DatabaseSessionStore, SessionRecord};

use talos_types::talos::{SessionSourceProvider, SessionSourceType};

use talos_types::{ModelRef, ModelSelection, ModelUsageKind, SessionConf, TalosConfig};

#[derive(Clone)]
struct TestModel {
    responses: Arc<Mutex<VecDeque<String>>>,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

async fn model_completion(
    axum::extract::State(model): axum::extract::State<TestModel>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Bearer worker-only-credential"
    );
    model.requests.lock().unwrap().push(request);
    let content = model
        .responses
        .lock()
        .unwrap()
        .pop_front()
        .expect("queued model response");
    axum::Json(serde_json::json!({
        "id":"worker-dream", "object":"chat.completion", "created":1, "model":"worker-inner",
        "choices":[{"index":0,"message":{"role":"assistant","content":content},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}
    }))
}

struct TestAuthService;

#[async_trait]
impl AuthService for TestAuthService {
    async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, _token: &str) -> TalosResult<RequestIdentity> {
        Ok(RequestIdentity::auth_disabled())
    }
}

#[tokio::test]
async fn dream_maintenance_uses_bound_worker_policy_for_two_user_scopes() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config =
        TalosConfig::default().with_workspace_definition(talos_config::WorkspaceDefinition::new(
            talos_config::DEFAULT_WORKSPACE_ID,
            workspace.path().to_path_buf(),
        ));
    let mut agent_kernel = config.agent.kernel.clone();
    agent_kernel.timezone = "Pacific/Kiritimati".to_string();
    config.agent.agents = vec![talos_config::AgentDefinitionConfig {
        id: "dream-agent".to_string(),
        name: "Dream Agent".to_string(),
        enabled: true,
        description: None,
        provider: Some(talos_config::AgentProviderConfig::Kernel {
            kernel: agent_kernel.clone(),
        }),
    }];
    config.llm.provider = "anthropic".to_string();
    config.llm.model = "default-maintenance-model".to_string();
    config.llm.anthropic.models = vec![
        talos_config::ProviderModelConfig {
            reasoning_replay: Default::default(),
            reasoning_efforts: None,
            request_parameters: Default::default(),
            id: config.llm.model.clone(),
            name: "Default maintenance model".to_string(),
            context_window: 32_768,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
        },
        talos_config::ProviderModelConfig {
            reasoning_replay: Default::default(),
            reasoning_efforts: None,
            request_parameters: Default::default(),
            id: "inner-maintenance-model".to_string(),
            name: "Inner maintenance model".to_string(),
            context_window: 32_768,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
        },
    ];
    config.agent.inner_model = Some(ModelRef {
        provider: "anthropic".to_string(),
        id: "inner-maintenance-model".to_string(),
    });
    let storage = Arc::new(SqliteStorage::new(":memory:").await.expect("storage"));
    let migration_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    talos_store::migration::run_migrations(storage.pool(), &migration_dir)
        .await
        .expect("test migrations should run");
    let sessions = Arc::new(
        DatabaseSessionStore::new(storage.clone())
            .await
            .expect("session store"),
    );
    let mut source_events = BTreeMap::new();
    for (session_id, user_id, last_activity) in [("s1", "u1", 2), ("s2", "u2", 1)] {
        let mut record = session_record(session_id, user_id, last_activity);
        if session_id == "s2" {
            record.conf.model_selection = None;
        }
        sessions
            .insert_session_record(&record)
            .await
            .expect("insert session");
        source_events.insert(
            session_id.to_string(),
            completed_history_events(
                session_id,
                &format!("turn-{session_id}"),
                &format!("message-{session_id}"),
                &format!("remember {user_id}"),
            ),
        );
    }
    let model = TestModel {
        responses: Arc::new(Mutex::new(VecDeque::from([
            candidate_response("first"),
            candidate_response("second"),
            candidate_response("failed-metrics"),
            "invalid formation".into(),
        ]))),
        requests: Default::default(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route("/chat/completions", axum::routing::post(model_completion))
        .with_state(model.clone());
    let model_server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let profiles = Arc::new(MockProfileStore::default());
    use talos_store::node::AgentNodeStore;
    let node_store = Arc::new(talos_store::node::DatabaseAgentNodeStore::new(&storage));
    let nodes = talos_service::ProductNodeAccess::computer(
        node_store
            .upsert_embedded(
                talos_types::NodeStatus::Enabled,
                chrono::Utc::now().timestamp(),
            )
            .await
            .unwrap()
            .into_info(),
        Arc::new(agent_protocol::NodeAdmissionGates::default()),
    );
    let project_store = Arc::new(
        StorageProjectStore::new(storage.clone())
            .await
            .expect("project store"),
    );
    let file_store = Arc::new(DatabaseFileStore::new(&storage));
    let file_operator = Operator::new(Memory::default())
        .expect("memory file operator")
        .finish();
    let file_service = Arc::new(
        talos_service::FileService::new(
            file_store,
            project_store.clone(),
            BTreeMap::from([("local".to_string(), file_operator)]),
            config.storage.files.clone(),
        )
        .expect("file service"),
    );
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(talos_router::RouterImpl::new()),
        sessions.clone(),
        node_store,
    ));
    let runtime_home = tempfile::tempdir().unwrap();
    let mut worker_config = config.clone();
    worker_config.agent.kernel.timezone = agent_kernel.timezone.clone();
    worker_config.llm = serde_json::from_value(serde_json::json!({
        "provider":"worker-local", "model":"worker-default", "custom_providers":{
            "worker-local":{"name":"Worker local model","protocol":"chat","base_url":endpoint,"api_key":"worker-only-credential",
            "models":[{"id":"worker-default"},{"id":"worker-inner"}]}
        }
    })).unwrap();
    worker_config.agent.inner_model = Some(ModelRef {
        provider: "worker-local".into(),
        id: "worker-inner".into(),
    });
    config.llm.anthropic.api_key = None;
    config.llm.anthropic.base_url = Some("http://127.0.0.1:9".into());
    // The control plane need not instantiate the Agent that owns this Session.
    config.agent.agents.clear();
    config.agent.kernel.timezone = "Pacific/Honolulu".into();
    let worker = talos_worker::TalosWorker::build(
        worker_config.with_runtime_paths(talos_config::TalosPaths::from_home(
            runtime_home.path().to_path_buf(),
        )),
        talos_worker::TalosWorkerResources {
            attachment_reader: file_service.clone(),
            tool_image_store: Arc::new(crate::tool_images::ToolImages(file_service.clone())),
            agent_control: None,
            automation_control: None,
        },
    )
    .await
    .unwrap();
    worker.use_local_resources({
        let data = crate::agent_node_resources_rpc::NodeSessionDataRpc::local(
            sessions.clone(),
            file_service.clone(),
        );
        let files = file_service.clone();
        move |context| {
            let data = data.clone();
            let files = files.clone();
            Box::pin(async move {
                data.authorize_local_session(&context).await?;
                Ok(talos_worker::TalosWorkerResources {
                    attachment_reader: files.clone(),
                    tool_image_store: Arc::new(crate::tool_images::ToolImages(files)),
                    agent_control: None,
                    automation_control: None,
                })
            })
        }
    });
    for (session_id, user) in [("s1", "u1"), ("s2", "u2")] {
        seed_worker_turn(
            &worker,
            sessions.as_ref(),
            &config,
            session_id,
            user,
            &format!("turn-{session_id}"),
            source_events.remove(session_id).unwrap(),
        )
        .await;
    }
    let (worker_api, worker_auth) = worker
        .start_local(
            talos_error::SessionOwnerKind::Server,
            talos_agent::SessionHold::Turn,
        )
        .await
        .unwrap();
    for (id, harness) in worker
        .local_harnesses(
            &worker_api,
            &worker_auth,
            "11111111-1111-4111-8111-111111111111",
        )
        .unwrap()
    {
        coordinator
            .register_agent("embedded".into(), id, harness)
            .await
            .unwrap();
    }
    let auth = Arc::new(TestAuthService);
    let service = Arc::new(talos_service::TalosService::new(
        coordinator.clone(),
        nodes,
        sessions.clone(),
        sessions.clone(),
        project_store,
        profiles.clone(),
        file_service,
        Arc::new(talos_service::TimelineEventService::new(sessions.clone())),
        config.clone(),
        auth,
        Arc::new(OpenModeUserService),
    ));
    let maintenance = DreamMemoryMaintenance::new(
        sessions.clone(),
        service,
        coordinator,
        profiles.clone(),
        &config,
        DreamMemoryMaintenanceConfig {
            batch_size: 1,
            max_batches: 2,
            ..DreamMemoryMaintenanceConfig::default()
        },
    );

    let report = maintenance.run().await.expect("daily maintenance");

    assert_eq!(
        report,
        DreamMemoryMaintenanceReport {
            processed_sessions: 2,
            failed_sessions: 0,
        }
    );
    assert!(!sessions.has_pending_dream_memory("s1").await.unwrap());
    assert!(!sessions.has_pending_dream_memory("s2").await.unwrap());
    for user_id in ["u1", "u2"] {
        let user_workspace = workspace.path().join(user_id);
        assert!(
            !user_workspace.join("MEMORY.md").exists(),
            "Dream Memory maintenance must not create User Workspace Memory"
        );
        let content = std::fs::read_to_string(user_workspace.join("memory/2026-07-17.md"))
            .expect("read formed Dream Memory");
        let context = DreamMemoryDocumentContext::new(
            MemoryAgentId::parse("dream-agent").expect("agent id"),
            MemoryUserId::parse(user_id).expect("user id"),
            DreamMemoryDate::parse("2026-07-17").expect("date"),
            MemoryTimezone::new(
                talos_config::parse_timezone(&agent_kernel.timezone).expect("timezone"),
            ),
        );
        let document =
            DreamMemoryDocument::parse(context, &content).expect("canonical Dream Memory");
        assert_eq!(document.entries().len(), 1);
        assert!(matches!(
            document.entries()[0].provenance(),
            DreamMemoryProvenance::Generated { .. }
        ));
    }

    let second_report = maintenance.run().await.expect("second daily maintenance");

    assert_eq!(
        second_report,
        DreamMemoryMaintenanceReport {
            processed_sessions: 0,
            failed_sessions: 0,
        }
    );
    let requested_models = model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|request| request["model"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(requested_models, vec!["worker-inner", "worker-inner"]);
    let expected_inner_model = ModelRef {
        provider: "worker-local".into(),
        id: "worker-inner".into(),
    };
    let usage_kinds = profiles
        .updates()
        .into_iter()
        .map(|update| match update {
            ProfileUsageUpdate::ModelCall(update) => (update.kind, update.model_ref),
            ProfileUsageUpdate::Turn(_) => panic!("maintenance must not record Turn usage"),
        })
        .collect::<Vec<_>>();
    assert!(usage_kinds
        .iter()
        .any(|(kind, _)| *kind == ModelUsageKind::Dream));
    assert!(usage_kinds
        .iter()
        .all(|(kind, _)| *kind == ModelUsageKind::Dream));
    assert!(usage_kinds
        .iter()
        .all(|(_, model_ref)| model_ref == &expected_inner_model));

    let u1_workspace = workspace.path().join("u1");
    let memory_path = u1_workspace.join("MEMORY.md");
    seed_worker_turn(
        &worker,
        sessions.as_ref(),
        &config,
        "s1",
        "u1",
        "turn-s1-2",
        completed_history_events(
            "s1",
            "turn-s1-2",
            "message-s1-2",
            "remember failure invariant",
        ),
    )
    .await;
    let cursor_before_failure = sessions.dream_memory_cursor("s1").await.unwrap();
    let record_before_failure = sessions
        .get_session_record("s1")
        .await
        .expect("load session")
        .expect("session record");
    profiles.set_record_usage_failure(true);

    maintenance
        .process_session(record_before_failure.clone())
        .await
        .expect_err("Dream Memory must fail when usage recording fails");
    assert_eq!(
        sessions.dream_memory_cursor("s1").await.unwrap(),
        cursor_before_failure
    );
    assert!(sessions.has_pending_dream_memory("s1").await.unwrap());
    assert_eq!(model.requests.lock().unwrap().len(), 3);
    profiles.set_record_usage_failure(false);
    assert!(maintenance
        .process_session(record_before_failure.clone())
        .await
        .unwrap());
    assert_eq!(
        model.requests.lock().unwrap().len(),
        3,
        "retry must replay Worker usage without another model call"
    );
    assert!(!maintenance
        .process_session(record_before_failure)
        .await
        .unwrap());
    assert_eq!(
        profiles.updates().len(),
        3,
        "consumed Worker usage must not be charged again"
    );
    let pending_cursor = sessions.dream_memory_cursor("s1").await.unwrap();
    seed_worker_turn(
        &worker,
        sessions.as_ref(),
        &config,
        "s1",
        "u1",
        "turn-invalid",
        completed_history_events(
            "s1",
            "turn-invalid",
            "message-invalid",
            "invalid formation fixture",
        ),
    )
    .await;
    let record = sessions.get_session_record("s1").await.unwrap().unwrap();
    let error = maintenance.process_session(record).await.unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::InternalError));
    assert_ne!(
        sessions.dream_memory_cursor("s1").await.unwrap(),
        pending_cursor
    );
    assert_eq!(
        profiles.updates().len(),
        4,
        "paid model usage survives formation failure"
    );
    let mut empty = session_record("empty-session", "u1", 0);
    empty.message_count = 0;
    sessions.insert_session_record(&empty).await.unwrap();
    assert!(!sessions.has_pending_dream_memory(&empty.id).await.unwrap());
    assert!(!maintenance.process_session(empty).await.unwrap());
    storage
        .execute(
            "DELETE FROM worker_event_projection WHERE session_id=?",
            &["s2"],
        )
        .await
        .unwrap();
    storage
        .execute(
            "DELETE FROM worker_projection_cursors WHERE session_id=?",
            &["s2"],
        )
        .await
        .unwrap();
    assert!(
        sessions.has_pending_dream_memory("s2").await.unwrap(),
        "missing product projections must remain conservative wake candidates"
    );
    model_server.abort();

    assert!(
        !memory_path.exists(),
        "Dream Memory maintenance failure must not create User Workspace Memory"
    );
}

fn completed_history_events(
    session_id: &str,
    turn_id: &str,
    message_id: &str,
    text: &str,
) -> Vec<AgentEvent> {
    let occurred_at = 1_784_251_800;
    let identity = AgentTurnRef::new(session_id, turn_id);
    let completed_block = AgentMessageBlock {
        block_id: format!("{message_id}-block"),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::UserContent(talos_agent_api::AgentUserContent::Text {
            text: text.to_string(),
        }),
        started_at: occurred_at,
        completed_at: Some(occurred_at + 1),
    };
    let completed_message = AgentMessage {
        message_id: message_id.to_string(),
        turn_id: turn_id.to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: vec![completed_block.clone()],
        created_at: occurred_at,
        completed_at: Some(occurred_at + 1),
    };
    let mut started_message = completed_message.clone();
    started_message.status = AgentMessageStatus::InProgress;
    started_message.blocks.clear();
    started_message.completed_at = None;
    let mut started_block = completed_block.clone();
    started_block.status = AgentMessageBlockStatus::Running;
    started_block.completed_at = None;
    let mut events = vec![
        AgentEvent {
            identity: identity.clone(),
            sequence: 1,
            occurred_at,
            payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        },
        AgentEvent {
            identity: identity.clone(),
            sequence: 2,
            occurred_at,
            payload: AgentEventPayload::MessageStarted(AgentMessageStarted {
                message: started_message,
            }),
        },
        AgentEvent {
            identity: identity.clone(),
            sequence: 3,
            occurred_at,
            payload: AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: message_id.to_string(),
                block: started_block,
            }),
        },
        AgentEvent {
            identity: identity.clone(),
            sequence: 4,
            occurred_at: occurred_at + 1,
            payload: AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
                message_id: message_id.to_string(),
                block: completed_block,
            }),
        },
        AgentEvent {
            identity: identity.clone(),
            sequence: 5,
            occurred_at: occurred_at + 1,
            payload: AgentEventPayload::MessageCompleted(completed_message),
        },
    ];
    events.push(AgentEvent {
        identity,
        sequence: events.last().unwrap().sequence + 1,
        occurred_at: occurred_at + 2,
        payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    });
    events
}

fn session_record(session_id: &str, user_id: &str, last_activity: u64) -> SessionRecord {
    SessionRecord {
        session_kind: talos_types::SessionKind::Standard,
        id: session_id.to_string(),
        agent_id: "dream-agent".to_string(),
        execution_node_id: "embedded".to_string(),
        tree_identity: talos_store::session::SessionTreeIdentity::Primary,
        workspace_id: "default".to_string(),
        title: "Memory test".to_string(),
        user_id: user_id.to_string(),
        source_type: SessionSourceType::System,
        source_provider: SessionSourceProvider::System,
        channel_id: None,
        thread_id: None,
        environment: None,
        created_at: 1,
        last_activity,
        message_count: 1,
        latest_turn_seq: 0,
        memory_processed_until_event_seq: 0,
        usage: talos_types::SessionUsage::default(),
        conf: SessionConf {
            model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                provider: "test".to_string(),
                id: "test-model".to_string(),
            })),
            ..SessionConf::default()
        },
        project_id: None,
        worktree_binding: None,
        current_directory: None,
        pinned: false,
    }
}

fn candidate_response(title: &str) -> String {
    format!(
        r#"{{"mode":"maintenance","memory_candidates":[{{"title":"{title}","body":"durable memory","kind":"fact","importance":"high"}}]}}"#
    )
}

async fn seed_worker_turn(
    worker: &talos_worker::TalosWorker,
    product: &DatabaseSessionStore,
    config: &TalosConfig,
    session_id: &str,
    user_id: &str,
    turn_id: &str,
    events: Vec<AgentEvent>,
) {
    let identity = AgentTurnRef::new(session_id, turn_id);
    let input = events
        .iter()
        .find_map(|event| match &event.payload {
            AgentEventPayload::MessageCompleted(message) => Some(message.clone()),
            _ => None,
        })
        .unwrap();
    let context = agent_protocol::AgentSessionContext {
        agent_id: "dream-agent".into(),
        session_id: session_id.into(),
        workspace: config.resolve_workspace("default", user_id).unwrap(),
    };
    let journal = worker.execution_store();
    journal
        .admit(
            &context,
            &agent_protocol::AgentExecutionCommand::Input {
                params: Box::new(agent_protocol::AgentInputParams {
                    command: None,
                    agent_id: "dream-agent".into(),
                    identity,
                    input: input.clone(),
                }),
            },
            vec![events[0].clone()],
            &agent_protocol::AgentRunCheckpoint {
                last_sequence: 1,
                messages: vec![],
                metrics: Default::default(),
            },
        )
        .await
        .unwrap();
    journal
        .append(
            &context,
            events[1..].to_vec(),
            &agent_protocol::AgentRunCheckpoint {
                last_sequence: events.last().unwrap().sequence,
                messages: vec![input],
                metrics: Default::default(),
            },
            Some(agent_protocol::AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: Default::default(),
            }),
        )
        .await
        .unwrap();
    let page = journal
        .read_events(agent_protocol::AgentSessionEventsParams {
            session: context,
            after_cursor: None,
            limit: agent_protocol::MAX_EXECUTION_PAGE_EVENTS,
        })
        .await
        .unwrap();
    product
        .project_worker_events(talos_store::session::WorkerProjectionParams {
            execution_node_id: "embedded".into(),
            session_id: session_id.into(),
            after_cursor: None,
            page,
        })
        .await
        .unwrap();
}
