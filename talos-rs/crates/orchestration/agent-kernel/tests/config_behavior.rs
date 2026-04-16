//! Feature tests for AgentKernelConfig behavior validation
//!
//! Tests verify default values and boundary conditions for kernel configuration.

use talos_config::{AgentKernelConfig, Config, PlannerMode, PlannerSelection};
use talos_error::TalosError;

/// Tests that AgentKernelConfig default values match the specification.
///
/// Default values per specification:
/// - max_iters: 1
/// - max_task_per_turn: 3
/// - tool_parallelism: 1
/// - max_llm_calls: 8
/// - max_tool_rounds: 8
/// - planner_mode: "built_in"
/// - planner_selection: "static"
#[test]
fn test_default_values() {
    let config = AgentKernelConfig::default();

    assert_eq!(config.max_iters, 1, "max_iters should default to 1");
    assert_eq!(
        config.max_task_per_turn, 3,
        "max_task_per_turn should default to 3"
    );
    assert_eq!(
        config.tool_parallelism, 1,
        "tool_parallelism should default to 1"
    );
    assert_eq!(config.max_llm_calls, 8, "max_llm_calls should default to 8");
    assert_eq!(
        config.max_tool_rounds, 8,
        "max_tool_rounds should default to 8"
    );

    assert_eq!(
        config.planner_mode,
        PlannerMode::BuiltIn,
        "planner_mode should default to 'built_in'"
    );
    assert_eq!(
        config.planner_selection,
        PlannerSelection::Static,
        "planner_selection should default to 'static'"
    );
}

/// Tests boundary validation for numeric configuration fields.
///
/// Valid boundaries:
/// - max_iters: must be > 0
/// - max_task_per_turn: must be > 0
/// - tool_parallelism: must be > 0
/// - max_llm_calls: must be > 0
/// - max_tool_rounds: must be > 0
/// - planner_mode: must be "built_in" or "plan_react" (None removed)
#[test]
fn test_boundary_validation() {
    let config = AgentKernelConfig::default();

    assert!(config.max_iters > 0, "max_iters must be > 0");
    assert!(
        config.max_task_per_turn > 0,
        "max_task_per_turn must be > 0"
    );
    assert!(config.tool_parallelism > 0, "tool_parallelism must be > 0");
    assert!(config.max_llm_calls > 0, "max_llm_calls must be > 0");
    assert!(config.max_tool_rounds > 0, "max_tool_rounds must be > 0");

    assert!(
        matches!(
            config.planner_mode,
            PlannerMode::BuiltIn | PlannerMode::PlanReAct
        ),
        "planner_mode must be a valid planner mode"
    );

    assert!(
        config.tool_result_max_bytes > 0,
        "tool_result_max_bytes must be > 0"
    );
    assert_eq!(
        config.tool_result_max_bytes,
        64 * 1024,
        "tool_result_max_bytes should default to 64KB"
    );

    assert!(
        config.context_compaction_max_bytes > 0,
        "context_compaction_max_bytes must be > 0"
    );
    assert_eq!(
        config.context_compaction_max_bytes,
        256 * 1024,
        "context_compaction_max_bytes should default to 256KB"
    );

    assert!(
        !config.default_workspace_id.trim().is_empty(),
        "default_workspace_id must be non-empty"
    );

    assert!(
        config.prompt.max_static_tokens > 0,
        "prompt.max_static_tokens must be > 0"
    );
    assert!(
        config.prompt.max_dynamic_tokens > 0,
        "prompt.max_dynamic_tokens must be > 0"
    );
    assert_eq!(
        config.prompt.max_static_tokens, 2000,
        "prompt.max_static_tokens should default to 2000"
    );
    assert_eq!(
        config.prompt.max_dynamic_tokens, 3200,
        "prompt.max_dynamic_tokens should default to 3200"
    );
    assert!(
        config.prompt.bootstrap.enabled,
        "prompt.bootstrap.enabled should default to true"
    );
    assert!(
        config.prompt.bootstrap.auto_create_missing_files,
        "prompt.bootstrap.auto_create_missing_files should default to true"
    );
    assert_eq!(
        config.prompt.bootstrap.max_chars_per_file, 8_000,
        "prompt.bootstrap.max_chars_per_file should default to 8000"
    );
    assert_eq!(
        config.prompt.bootstrap.max_chars_total, 24_000,
        "prompt.bootstrap.max_chars_total should default to 24000"
    );

    assert!(
        !config.memory.workspace_dir.trim().is_empty(),
        "memory.workspace_dir must be non-empty"
    );
    assert!(
        config.memory.hot_recent_messages > 0,
        "memory.hot_recent_messages must be > 0"
    );
    assert!(
        config.memory.session_recent_messages_limit > 0,
        "memory.session_recent_messages_limit must be > 0"
    );
    assert_eq!(
        config.memory.root_memory_filename, "MEMORY.md",
        "memory.root_memory_filename should be fixed to MEMORY.md"
    );

    assert!(
        !config.binding.default_agent_kind.trim().is_empty(),
        "binding.default_agent_kind must be non-empty"
    );
    assert_eq!(
        config.binding.default_agent_kind, "llm",
        "binding.default_agent_kind should default to 'llm'"
    );
}

/// Tests that nested config structures are properly initialized.
#[test]
fn test_nested_config_defaults() {
    let config = AgentKernelConfig::default();

    assert!(config.prompt.include_session_summary);
    assert!(config.prompt.include_recent_messages);
    assert!(config.prompt.bootstrap.include_memory);

    assert!(config.memory.session_summary_enabled);
    assert!(config.memory.workspace_memory_flush_enabled);
}

/// Tests that config can be cloned and remains independent.
#[test]
fn test_config_clone() {
    let original = AgentKernelConfig::default();
    let cloned = original.clone();

    assert_eq!(original.max_iters, cloned.max_iters);
    assert_eq!(original.planner_mode, cloned.planner_mode);
    assert_eq!(original.planner_selection, cloned.planner_selection);
}

#[test]
fn test_bootstrap_char_budget_relationship_validation() {
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.max_chars_per_file = 8_000;
    config.prompt.bootstrap.max_chars_total = 4_000;

    let err = Config {
        agent: talos_config::AgentConfig {
            kernel: config,
            ..talos_config::AgentConfig::default()
        },
        ..Config::default()
    }
    .validate_runtime()
    .expect_err("invalid bootstrap char budget should fail");

    assert!(matches!(err, TalosError::InvalidConfiguration(_)));
}
