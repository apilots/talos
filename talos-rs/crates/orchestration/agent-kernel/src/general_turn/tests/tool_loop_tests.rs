use super::*;
use crate::StateNamespace;
use talos_prompt::SessionContinuity;

#[tokio::test]

async fn test_run_general_turn_bash_then_task_output() {
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
        "Use Bash, then continue with TaskOutput.",
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
        "Use Bash with escalated permissions.",
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

    let continuity_value = ctx
        .session
        .state
        .get(StateNamespace::Session, SESSION_CONTINUITY_STATE_KEY)
        .cloned()
        .expect("session continuity should be staged after the turn");
    let continuity: SessionContinuity =
        serde_json::from_value(continuity_value).expect("continuity should parse");
    assert_eq!(
        continuity.recent_goal,
        "Use Bash with escalated permissions."
    );
    assert_eq!(
        continuity.last_failure.as_deref(),
        Some("Bash returned an error")
    );
    assert_eq!(
        continuity.suggested_next_step.as_deref(),
        Some("adjust the failing path, command, or tool arguments before retrying")
    );
    assert_eq!(
        continuity.pending_tasks,
        vec!["fix the failing tool step and retry".to_string()]
    );
}
