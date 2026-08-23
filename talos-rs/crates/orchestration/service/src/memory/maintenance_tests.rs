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
use talos_storage::SqliteStorage;
use talos_store::profile::{MockProfileStore, ProfileUsageUpdate};
use talos_store::session::{DatabaseSessionStore, SessionArtifactStore, SessionRecord, TalosTurn};
use talos_types::talos::{
    RunItem, StopReason, TalosContext, TalosProvider, TalosSourceType, TokenUsage,
};
use talos_types::{
    resolve_default_model_selection, resolve_model_catalog, ModelRef, ModelSelection,
    ModelUsageKind, SessionConf,
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

#[test]
fn managed_update_preserves_user_content() {
    let current = format!("# My Memory\n\nUser note.\n\n{MANAGED_START}\n- old\n{MANAGED_END}\n");

    let updated = replace_managed_content(&current, "- new").expect("managed update");

    assert_eq!(
        updated,
        format!("# My Memory\n\nUser note.\n\n{MANAGED_START}\n- new\n{MANAGED_END}\n")
    );
}

#[test]
fn managed_update_rejects_unpaired_marker() {
    let error = replace_managed_content(MANAGED_START, "- new").expect_err("invalid markers");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn daily_maintenance_processes_two_user_scopes() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = workspace.path().to_string_lossy().to_string();
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
        let kernel_config = resolve_effective_kernel_workspace(&config.agent.kernel, user_id)
            .expect("user workspace")
            .kernel_config;
        artifacts
            .append_turn(
                std::path::Path::new(&kernel_config.workspace_dir),
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
            "- managed memory".to_string(),
            "- managed memory".to_string(),
            "- managed memory".to_string(),
            "- managed memory".to_string(),
            candidate_response("failed-metrics"),
            "- changed memory".to_string(),
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
    let maintenance = MemoryDailyMaintenance::new(
        sessions.clone(),
        sessions.clone(),
        artifacts.clone(),
        memory,
        inner_model,
        config.clone(),
        MemoryDailyMaintenanceConfig {
            batch_size: 1,
            max_batches: 2,
            ..MemoryDailyMaintenanceConfig::default()
        },
    );

    let report = maintenance.run().await.expect("daily maintenance");

    assert_eq!(
        report,
        MemoryDailyMaintenanceReport {
            processed_sessions: 2,
            failed_sessions: 0,
            updated_users: 2,
            failed_users: 0,
        }
    );
    for user_id in ["u1", "u2"] {
        let content = std::fs::read_to_string(
            workspace
                .path()
                .join("users")
                .join(user_id)
                .join("MEMORY.md"),
        )
        .expect("managed memory");
        assert_eq!(
            content,
            format!("{MANAGED_START}\n- managed memory\n{MANAGED_END}\n")
        );
    }

    let second_report = maintenance.run().await.expect("second daily maintenance");

    assert_eq!(
        second_report,
        MemoryDailyMaintenanceReport {
            processed_sessions: 0,
            failed_sessions: 0,
            updated_users: 2,
            failed_users: 0,
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
        .any(|(kind, _)| *kind == ModelUsageKind::Memory));
    assert!(usage_kinds
        .iter()
        .all(|(_, model_ref)| model_ref == &expected_inner_model));

    let u1_workspace = workspace.path().join("users").join("u1");
    let memory_path = u1_workspace.join("MEMORY.md");
    let memory_before_failure = std::fs::read_to_string(&memory_path).expect("managed memory");
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
        .expect_err("daily memory must fail when usage recording fails");
    let record_after_failure = sessions
        .get_session_record("s1")
        .await
        .expect("reload session")
        .expect("session record");
    assert_eq!(
        record_after_failure.memory_processed_until_seq,
        record_before_failure.memory_processed_until_seq
    );

    maintenance
        .refresh_managed_memory("u1")
        .await
        .expect_err("managed memory must fail when usage recording fails");
    assert_eq!(
        std::fs::read_to_string(memory_path).expect("managed memory after failure"),
        memory_before_failure
    );
}

fn session_record(session_id: &str, user_id: &str, last_activity: u64) -> SessionRecord {
    SessionRecord {
        id: session_id.to_string(),
        agent_id: "talos".to_string(),
        tree_identity: talos_store::session::SessionTreeIdentity::Primary,
        workspace_id: None,
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
        r#"{{"memory_candidates":[{{"title":"{title}","body":"durable memory","kind":"fact","importance":"high"}}]}}"#
    )
}
