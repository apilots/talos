use super::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::Mutex;
use talos_llm_provider::{
    ChatChunk, ChatResponse, ChatStream, LlmProvider, ReasoningOutput, Usage,
};
use talos_storage::SqliteStorage;
use talos_store::session::{DatabaseSessionStore, SessionArtifactStore, SessionRecord, TalosTurn};
use talos_types::talos::{
    RunItem, StopReason, TalosContext, TalosProvider, TalosSourceType, TokenUsage,
};
use talos_types::{ModelRef, SessionConf};
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
}

impl LlmProviderRouter for TestRouter {
    fn provider_for(&self, _model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
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

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[tokio::test]
async fn daily_maintenance_processes_two_user_scopes() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = workspace.path().to_string_lossy().to_string();
    let storage = Arc::new(SqliteStorage::new(":memory:").await.expect("storage"));
    let sessions = Arc::new(
        DatabaseSessionStore::new(storage)
            .await
            .expect("session store"),
    );
    let artifacts = Arc::new(talos_store::session::FileSessionArtifactStore::default());
    for (session_id, user_id, last_activity) in [("s1", "u1", 2), ("s2", "u2", 1)] {
        sessions
            .insert_session_record(
                &session_record(session_id, user_id, last_activity),
                "talos",
                "test",
            )
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
        ])),
    });
    let maintenance = MemoryDailyMaintenance::new(
        sessions.clone(),
        sessions.clone(),
        artifacts,
        memory,
        Arc::new(TestRouter { provider }),
        config,
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
}

fn session_record(session_id: &str, user_id: &str, last_activity: u64) -> SessionRecord {
    SessionRecord {
        id: session_id.to_string(),
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
        usage: TokenUsage::default(),
        conf: SessionConf {
            model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                provider: "test".to_string(),
                id: "test-model".to_string(),
            })),
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
