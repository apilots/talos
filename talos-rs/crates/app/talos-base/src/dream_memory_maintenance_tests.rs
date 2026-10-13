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
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter,
    ReasoningOutput, Usage,
};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryProvenance,
    MemoryAgentId, MemoryTimezone, MemoryUserId,
};
use talos_storage::SqliteStorage;
use talos_store::file::DatabaseFileStore;
use talos_store::profile::{MockProfileStore, ProfileUsageUpdate};
use talos_store::project::StorageProjectStore;
use talos_store::session::{
    AgentExecutionStore, CommitInitialAdmissionParams, CommitTerminalSettlementParams,
    DatabaseSessionStore, SessionRecord,
};
use talos_types::talos::{SessionSourceProvider, SessionSourceType};

use talos_types::{
    resolve_default_model_selection, resolve_model_catalog, ModelRef, ModelSelection,
    ModelUsageKind, SessionConf, TalosConfig,
};
use tokio::sync::mpsc;

struct TestProvider {
    responses: Mutex<VecDeque<String>>,
}

#[async_trait]
impl LlmProvider for TestProvider {
    fn name(&self) -> &str {
        "memory-maintenance-test"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: self
                .responses
                .lock()
                .expect("provider responses")
                .pop_front()
                .expect("queued provider response"),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "test-model".to_string()
    }
}

struct TestRouter {
    provider: Arc<dyn LlmProvider>,
    requested_model_refs: Mutex<Vec<ModelRef>>,
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

impl LlmProviderRouter for TestRouter {
    fn provider_for(&self, model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        self.requested_model_refs
            .lock()
            .expect("requested model refs")
            .push(model_ref.clone());
        Ok(self.provider.clone())
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
    let default_selection =
        resolve_default_model_selection(&config.llm).expect("default model selection");
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
    let memory_progress = Arc::new(crate::memory_progress::SessionMemoryProgress::new(
        sessions.clone(),
    ));
    let history = Arc::new(talos_service::SessionHistoryService::new(Arc::new(
        talos_service::TimelineEventService::new(sessions.clone()),
    )));
    for (session_id, user_id, last_activity) in [("s1", "u1", 2), ("s2", "u2", 1)] {
        let mut record = session_record(session_id, user_id, last_activity);
        if session_id == "s2" {
            record.conf.model_selection = None;
        }
        sessions
            .insert_session_record(&record)
            .await
            .expect("insert session");
        commit_completed_history_turn(
            sessions.as_ref(),
            session_id,
            &format!("turn-{session_id}"),
            &format!("message-{session_id}"),
            &format!("remember {user_id}"),
        )
        .await;
    }
    let provider: Arc<dyn LlmProvider> = Arc::new(TestProvider {
        responses: Mutex::new(VecDeque::from([
            candidate_response("first"),
            candidate_response("second"),
            candidate_response("failed-metrics"),
        ])),
    });
    let router = Arc::new(TestRouter {
        provider,
        requested_model_refs: Mutex::new(Vec::new()),
    });
    let profiles = Arc::new(MockProfileStore::default());
    let inner_model = Arc::new(
        talos_service::InnerModelClient::builder()
            .configured_model(config.agent.inner_model.clone())
            .agent_default_model(default_selection.model_ref.clone())
            .model_catalog(resolve_model_catalog(&config.llm))
            .provider_router(router.clone())
            .profile_store(profiles.clone())
            .profile_timezone("UTC".to_string())
            .build()
            .expect("inner model client"),
    );
    let nodes = talos_service::AgentNodeService::new(Arc::new(
        talos_store::node::DatabaseAgentNodeStore::new(storage.pool().clone()),
    ));
    let project_store = Arc::new(
        StorageProjectStore::new(storage.clone())
            .await
            .expect("project store"),
    );
    let file_store = Arc::new(DatabaseFileStore::new(storage.pool().clone()));
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
        Arc::new(talos_store::node::DatabaseAgentNodeStore::new(
            storage.pool().clone(),
        )),
    ));
    let runtime_home = tempfile::tempdir().unwrap();
    let mut worker_config = config.clone();
    worker_config.agent.kernel.timezone = agent_kernel.timezone.clone();
    // The control plane need not instantiate the Agent that owns this Session.
    config.agent.agents.clear();
    config.agent.kernel.timezone = "Pacific/Honolulu".into();
    let worker = talos_worker::TalosWorker::build(
        worker_config.with_runtime_paths(talos_config::TalosPaths::from_home(
            runtime_home.path().to_path_buf(),
        )),
        talos_worker::TalosWorkerResources {
            session_history_reader: history,
            memory_progress_store: memory_progress,
            attachment_reader: file_service.clone(),
            tool_image_store: Arc::new(crate::tool_images::ToolImages(file_service.clone())),
            agent_control: None,
        },
    )
    .await
    .unwrap();
    for (id, harness) in worker.harnesses() {
        coordinator
            .register_agent("embedded".into(), id.clone(), harness.clone())
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
        inner_model,
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
    let requested_model_refs = router
        .requested_model_refs
        .lock()
        .expect("requested model refs")
        .clone();
    assert!(!requested_model_refs.is_empty());
    let expected_inner_model = config.agent.inner_model.unwrap();
    let usage_kinds = profiles
        .updates()
        .into_iter()
        .map(|update| match update {
            ProfileUsageUpdate::ModelCall(update) => (update.kind, update.model_ref),
            ProfileUsageUpdate::Turn(_) => panic!("maintenance must not record Turn usage"),
        })
        .collect::<Vec<_>>();
    assert!(requested_model_refs
        .iter()
        .all(|model_ref| model_ref == &expected_inner_model));
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
    commit_completed_history_turn(
        sessions.as_ref(),
        "s1",
        "turn-s1-2",
        "message-s1-2",
        "remember failure invariant",
    )
    .await;
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
    let record_after_failure = sessions
        .get_session_record("s1")
        .await
        .expect("reload session")
        .expect("session record");
    assert_eq!(
        record_after_failure.memory_processed_until_event_seq,
        record_before_failure.memory_processed_until_event_seq
    );

    assert!(
        !memory_path.exists(),
        "Dream Memory maintenance failure must not create User Workspace Memory"
    );
}

async fn commit_completed_history_turn(
    store: &dyn AgentExecutionStore,
    session_id: &str,
    turn_id: &str,
    message_id: &str,
    text: &str,
) {
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
    let admission = store
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: vec![
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
            ],
            command: None,
        })
        .await
        .expect("canonical Turn admission should commit");
    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity,
                sequence: 6,
                occurred_at: occurred_at + 2,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("canonical Turn settlement should commit");
}

fn session_record(session_id: &str, user_id: &str, last_activity: u64) -> SessionRecord {
    SessionRecord {
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
        current_directory: None,
    }
}

fn candidate_response(title: &str) -> String {
    format!(
        r#"{{"mode":"maintenance","memory_candidates":[{{"title":"{title}","body":"durable memory","kind":"fact","importance":"high"}}]}}"#
    )
}
