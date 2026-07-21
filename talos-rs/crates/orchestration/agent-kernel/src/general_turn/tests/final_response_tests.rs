use super::*;

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
    let memory_store = Arc::new(TestMemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
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
        skill_options_from_config(&workspace_dir, &config),
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
    assert!(
        provider
            .finalization_context_had_tool_result
            .load(Ordering::SeqCst),
        "forced summary should be grounded in prior tool results"
    );
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
    let memory_store = Arc::new(TestMemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
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
        skill_options_from_config(&workspace_dir, &config),
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
