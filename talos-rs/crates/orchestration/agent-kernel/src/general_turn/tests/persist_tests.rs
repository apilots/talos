use super::*;
use crate::{
    InvocationSession, KernelTranscriptEvent, KernelTranscriptFilter, KernelTranscriptService,
};

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

    async fn list_events(
        &self,
        _session_id: &str,
        _filter: &KernelTranscriptFilter,
    ) -> TalosResult<Vec<KernelTranscriptEvent>> {
        unreachable!("not used by this test")
    }
}

#[tokio::test]
async fn general_turn_returns_the_persisted_artifact_sequence() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("temp workspace");
    let memory_store = Arc::new(TestMemoryStore::new(workspace.clone()));
    let tool_registry = Arc::new(ToolRegistry::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace);
    let mut ctx = build_ctx_with_stream_for_session("persisted-sequence", None);

    let first = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "hello",
        Arc::new(NoopLlmProvider),
        tool_registry.clone(),
        tool_registry.clone(),
        memory_store.clone(),
        &config,
        skill_options_from_config(&workspace, &config),
    )
    .await
    .expect("first turn should succeed");
    let second = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "again",
        Arc::new(NoopLlmProvider),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace, &config),
    )
    .await
    .expect("second turn should succeed");

    assert_eq!((first.artifact_seq, second.artifact_seq), (1, 2));
}

#[tokio::test]
async fn general_turn_returns_artifact_append_errors() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("temp workspace");
    let memory_store = Arc::new(TestMemoryStore::new(workspace.clone()));
    let tool_registry = Arc::new(ToolRegistry::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace);
    let mut ctx = build_ctx_with_stream_for_session("failed-persistence", None);
    ctx.message_id.clear();

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
    .expect_err("artifact append error should fail the turn");

    assert!(error.to_string().contains("message_id"), "{error}");
}

#[tokio::test]
async fn session_state_failure_happens_before_artifact_append() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("temp workspace");
    let memory_store = Arc::new(TestMemoryStore::new(workspace.clone()));
    let tool_registry = Arc::new(ToolRegistry::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace);
    let mut ctx = build_ctx_with_stream_for_session("failed-state-update", None);
    ctx.transcript_service = Some(Arc::new(FailingTranscriptService));

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
    let turns = talos_store::session::FileSessionArtifactStore::default()
        .load_turns(&workspace, "failed-state-update")
        .await
        .expect("turn artifacts should be readable");

    assert!(error.to_string().contains("injected transcript failure"));
    assert!(turns.is_empty());
}
