use super::*;
use crate::test_support::run_test_migrations;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter,
    ReasoningOutput, Usage,
};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryProvenance,
    MemoryAgentId, MemoryTimezone, MemoryUserId,
};
use talos_storage::SqliteStorage;
use talos_store::profile::{MockProfileStore, ProfileUsageUpdate};
use talos_store::session::{DatabaseSessionStore, SessionArtifactStore, SessionRecord, TalosTurn};
use talos_types::talos::{
    RunItem, StopReason, TalosContext, TalosProvider, TalosSourceType, TokenUsage,
};
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
async fn dream_maintenance_processes_two_user_scopes() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config =
        TalosConfig::default().with_workspace_definition(talos_config::WorkspaceDefinition::new(
            talos_config::DEFAULT_WORKSPACE_ID,
            workspace.path().to_path_buf(),
        ));
    let mut agent_kernel = config.agent.kernel.clone();
    agent_kernel.timezone = "Asia/Shanghai".to_string();
    config.agent.agents = vec![talos_config::AgentDefinitionConfig {
        id: "dream-agent".to_string(),
        name: "Dream Agent".to_string(),
        kind: talos_config::AgentKind::Kernel,
        enabled: true,
        description: None,
        runtime_config: Some(talos_config::AgentRuntimeConfig::Kernel {
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
    let agent_definitions =
        talos_agent::AgentDefinitionLoader::load(&config.agent, &default_selection)
            .expect("agent definitions");
    let storage = Arc::new(SqliteStorage::new(":memory:").await.expect("storage"));
    run_test_migrations(storage.pool()).await;
    let sessions = Arc::new(
        DatabaseSessionStore::new(storage)
            .await
            .expect("session store"),
    );
    let artifacts = Arc::new(talos_store::session::FileSessionArtifactStore::default());
    for (session_id, user_id, last_activity) in [("s1", "u1", 2), ("s2", "u2", 1)] {
        let mut record = session_record(session_id, user_id, last_activity);
        if session_id == "s2" {
            record.conf.model_selection = None;
        }
        sessions
            .insert_session_record(&record, "test")
            .await
            .expect("insert session");
        let effective_workspace = config
            .resolve_workspace("default", user_id)
            .expect("user workspace");
        artifacts
            .append_turn(
                &effective_workspace.root,
                &TalosTurn {
                    turn_id: format!("turn-{session_id}"),
                    message_id: format!("message-{session_id}"),
                    session_id: session_id.to_string(),
                    seq: 1,
                    timestamp: DateTime::<Utc>::from_timestamp(1_784_251_800, 0)
                        .expect("timestamp"),
                    items: vec![RunItem::UserInput {
                        text: format!("remember {user_id}"),
                        file_ids: Vec::new(),
                    }],
                    usage: TokenUsage::default(),
                    stop_reason: StopReason::EndTurn,
                },
            )
            .await
            .expect("append turn");
    }
    let memory = MemoryService::new(
        config.clone(),
        Arc::new(
            talos_store::memory::MemoryStore::in_memory()
                .await
                .expect("memory store"),
        ),
    )
    .expect("memory service");
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
        talos_agent::InnerModelClient::builder()
            .configured_model(config.agent.inner_model.clone())
            .agent_default_model(default_selection.model_ref.clone())
            .model_catalog(resolve_model_catalog(&config.llm))
            .provider_router(router.clone())
            .profile_store(profiles.clone())
            .profile_timezone("UTC".to_string())
            .build()
            .expect("inner model client"),
    );
    let maintenance = DreamMemoryMaintenance::new(
        sessions.clone(),
        sessions.clone(),
        artifacts.clone(),
        memory,
        inner_model,
        &agent_definitions,
        config.workspaces.clone(),
        config.talos_paths(),
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
    artifacts
        .append_turn(
            &u1_workspace,
            &TalosTurn {
                turn_id: "turn-s1-2".to_string(),
                message_id: "message-s1-2".to_string(),
                session_id: "s1".to_string(),
                seq: 2,
                timestamp: DateTime::<Utc>::from_timestamp(1_784_251_801, 0).expect("timestamp"),
                items: vec![RunItem::UserInput {
                    text: "remember failure invariant".to_string(),
                    file_ids: Vec::new(),
                }],
                usage: TokenUsage::default(),
                stop_reason: StopReason::EndTurn,
            },
        )
        .await
        .expect("append failure turn");
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
        record_after_failure.memory_processed_until_seq,
        record_before_failure.memory_processed_until_seq
    );

    assert!(
        !memory_path.exists(),
        "Dream Memory maintenance failure must not create User Workspace Memory"
    );
}

fn session_record(session_id: &str, user_id: &str, last_activity: u64) -> SessionRecord {
    SessionRecord {
        id: session_id.to_string(),
        agent_id: "dream-agent".to_string(),
        tree_identity: talos_store::session::SessionTreeIdentity::Primary,
        workspace_id: "default".to_string(),
        title: "Memory test".to_string(),
        user_id: user_id.to_string(),
        context: TalosContext {
            source_type: TalosSourceType::System,
            provider: TalosProvider::System,
            user_id: user_id.to_string(),
            message_id: format!("message-{session_id}"),
            trace_id: format!("trace-{session_id}"),
            channel_id: None,
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 1,
        last_activity,
        message_count: 1,
        latest_turn_seq: 0,
        memory_processed_until_seq: 0,
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
