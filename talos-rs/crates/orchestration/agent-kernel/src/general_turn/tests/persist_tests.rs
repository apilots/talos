use super::*;
use crate::{InvocationSession, KernelTranscriptEvent, KernelTranscriptService};

struct FailingTranscriptService;

#[async_trait]
impl KernelTranscriptService for FailingTranscriptService {
    async fn append_event(
        &self,
        _session: &mut InvocationSession,
        _event: KernelTranscriptEvent,
    ) -> TalosResult<KernelTranscriptEvent> {
        Err(TalosError::internal(
            "injected transcript failure".to_string(),
        ))
    }

    async fn load_session(
        &self,
        _session_id: &str,
        _invocation_id: Option<&str>,
        _branch: Option<crate::ExecutionBranch>,
    ) -> TalosResult<InvocationSession> {
        unreachable!("not used by this test")
    }
}

#[tokio::test]
async fn session_state_failure_stops_the_turn() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("temp workspace");
    let memory_store = Arc::new(TestMemoryStore::new());
    let tool_registry = Arc::new(ToolRegistry::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace);
    let mut ctx = build_ctx_with_stream_for_session("failed-state-update", None);
    ctx.transcript_service = Arc::new(FailingTranscriptService);

    let error = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "hello",
        Arc::new(NoopLlmProvider),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace, &config),
    )
    .await
    .expect_err("session state update should fail the turn");
    assert!(error.to_string().contains("injected transcript failure"));
}
