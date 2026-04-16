use super::*;
use talos_prompt::PromptMode;

#[test]
fn test_build_general_turn_step_request_respects_parallel_flag() {
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        project_root: None,
        selected_model: "model".to_string(),
        prompt_prefix_messages: Vec::new(),
        messages: Vec::new(),
        tool_definitions: Vec::new(),
        prompt_diagnostics: PromptDiagnostics {
            mode: PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            truncated_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
        },
        warnings: Vec::new(),
        prompt_warning_state: None,
        stream_tx: None,
    };
    let state = GeneralTurnLoopState::new(&prepared);

    let sequential_request = build_general_turn_step_request(&prepared, &state, false);
    assert!(!sequential_request.parallel_tool_calls);

    let parallel_request = build_general_turn_step_request(&prepared, &state, true);
    assert!(parallel_request.parallel_tool_calls);
}

#[tokio::test]
async fn test_run_general_turn_emits_prompt_warning_event() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    seed_recent_user_message(memory_store.as_ref()).await;
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.prompt.max_dynamic_tokens = 1;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "hello",
        provider,
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert!(!result.warnings.is_empty());
    assert!(ctx.events.iter().any(|event| {
        matches!(event.event_type, KernelEventType::PromptWarning)
            && matches!(event.label, KernelEventLabel::PromptWarning)
            && matches!(event.data, KernelEventData::PromptWarning(_))
    }));
}

#[tokio::test]
async fn test_run_general_turn_streams_prompt_warning_event() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    seed_recent_user_message(memory_store.as_ref()).await;
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.prompt.max_dynamic_tokens = 1;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
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
    )
    .await
    .expect("general turn should succeed");

    let mut saw_prompt_warning = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event.event_type, KernelStreamEventType::PromptWarning)
            && matches!(event.data, KernelStreamEventData::PromptWarning(_))
        {
            saw_prompt_warning = true;
            break;
        }
    }
    assert!(saw_prompt_warning, "stream should include prompt.warning");
}

#[tokio::test]
async fn test_run_general_turn_exec_command_then_write_stdin() {
    let provider = Arc::new(ExecLoopProvider::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use exec_command, then continue with write_stdin.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert!(result.output_text.contains("done:ok"));
    assert!(result
        .output_items
        .iter()
        .any(|item| matches!(item, talos_types::talos::TalosOutputItem::ToolResult { .. })));
    assert!(provider.round.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn test_run_general_turn_stops_on_pending_tool_confirmation() {
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let provider = Arc::new(ApprovalLoopProvider::default());
    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use exec_command with escalated permissions.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(result.output_text, "Tool confirmation pending.");
    assert!(result.output_items.iter().any(|item| matches!(
        item,
        talos_types::talos::TalosOutputItem::ToolResult { is_error: true, .. }
    )));
    assert_eq!(provider.round.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_run_general_turn_forces_summary_when_final_response_is_empty_after_tools() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(workspace_dir.join("Cargo.toml"), "[workspace]\n")
        .expect("fixture should exist");

    let provider = Arc::new(EmptyFinalAfterToolsProvider {
        dir_path: workspace_dir.to_string_lossy().to_string(),
        ..Default::default()
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Analyze the project architecture from code.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(
        result.output_text,
        "Final architecture summary based on verified tool results."
    );
    assert!(
        !result.output_text.contains("Tool execution completed."),
        "forced summary should replace placeholder fallback"
    );
    assert!(provider.round.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn test_run_general_turn_uses_salvage_call_when_forced_summary_is_still_empty() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(workspace_dir.join("Cargo.toml"), "[workspace]\n")
        .expect("fixture should exist");

    let provider = Arc::new(SalvageAfterEmptyFinalProvider {
        dir_path: workspace_dir.to_string_lossy().to_string(),
        ..Default::default()
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Analyze the project architecture from code.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(
        result.output_text,
        "Salvaged final answer from verified tool results."
    );
    assert!(
        !result.output_text.contains("Tool execution completed."),
        "salvage call should replace placeholder fallback"
    );
    assert!(provider.round.load(Ordering::SeqCst) >= 4);
}
