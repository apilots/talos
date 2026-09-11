use super::*;
use talos_prompt::{PromptMode, PromptWindowUsage};

#[tokio::test]

async fn test_execute_general_turn_loop_reports_model_output_limit() {
    let provider = Arc::new(LengthFinishProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "default".to_string(),
        project_root: None,
        selected_model: "length-finish".to_string(),
        max_output_tokens: Some(16),
        reasoning_effort: None,
        speed: Default::default(),
        attachment_tokens: 0,
        image_input_enabled: false,
        messages: Vec::new(),
        maintenance_usage: TokenUsage::default(),
        tool_selection_request: talos_tools::ToolSelectionRequest::default().with_session_id("s1"),
        prompt_diagnostics: PromptDiagnostics {
            mode: PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
            window_usage: PromptWindowUsage::default(),
            session_history_compaction: None,
        },
        prompt_warning_state: None,
        stream_tx: None,
        message_rx: None,
        run_tasks: crate::test_run_tasks(),
    };

    let result = execute_general_turn_loop(
        prepared,
        provider,
        tool_registry.clone(),
        &AgentKernelConfig::default(),
    )
    .await
    .expect("general turn should complete");

    assert_eq!(result.stop_reason, StopReason::ModelOutputLimit);
    assert_eq!(result.output_text, "partial answer");
}

#[tokio::test]
async fn test_run_general_turn_streams_prompt_warning_event() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(
        workspace_dir.join("SOUL.md"),
        "# SOUL\n\n- Keep the run grounded in repo evidence.\n",
    )
    .expect("soul file should exist");
    let memory_store = Arc::new(TestMemoryStore::new());
    seed_recent_user_message(memory_store.as_ref()).await;
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.auto_create_missing_files = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let (tx, mut rx) = mpsc::channel(16);
    let mut ctx = build_ctx_with_stream(Some(tx));

    let _ = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "hello",
        provider,
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("general turn should succeed");

    let mut saw_prompt_warning = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event.payload, AgentKernelEventPayload::PromptWarning(_)) {
            saw_prompt_warning = true;
            break;
        }
    }
    assert!(saw_prompt_warning, "stream should include prompt.warning");
}
