//! Feature tests for AgentKernelConfig behavior validation
//!
//! Tests verify default values and boundary conditions for kernel configuration.

use talos_config::AgentKernelConfig;

/// Tests that AgentKernelConfig default values match the specification.
///
/// Default values per specification:
/// - max_iters: 1
/// - max_task_per_turn: 3
/// - tool_parallelism: 4
/// - max_llm_calls: 8
/// - max_tool_rounds: 8
/// - planner_mode: "built_in"
/// - allow_request_planner_override: false
#[test]
fn test_default_values() {
    let config = AgentKernelConfig::default();

    assert_eq!(config.max_iters, 1, "max_iters should default to 1");
    assert_eq!(
        config.max_task_per_turn, 3,
        "max_task_per_turn should default to 3"
    );
    assert_eq!(
        config.tool_parallelism, 4,
        "tool_parallelism should default to 4"
    );
    assert_eq!(config.max_llm_calls, 8, "max_llm_calls should default to 8");
    assert_eq!(
        config.max_tool_rounds, 8,
        "max_tool_rounds should default to 8"
    );

    assert_eq!(
        config.planner_mode, "built_in",
        "planner_mode should default to 'built_in'"
    );
    assert!(
        !config.allow_request_planner_override,
        "allow_request_planner_override should default to false"
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
/// - planner_mode: must be "built_in" or "plan_react"
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

    let valid_modes = ["built_in", "plan_react"];
    assert!(
        valid_modes.contains(&config.planner_mode.as_str()),
        "planner_mode must be one of: {:?}",
        valid_modes
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
        config.prompt.max_static_tokens, 1200,
        "prompt.max_static_tokens should default to 1200"
    );
    assert_eq!(
        config.prompt.max_dynamic_tokens, 3200,
        "prompt.max_dynamic_tokens should default to 3200"
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
        config.memory.context_target_messages > 0,
        "memory.context_target_messages must be > 0"
    );
    assert!(
        (0.0..=1.0).contains(&config.memory.long_term_min_importance),
        "memory.long_term_min_importance must be in [0.0, 1.0]"
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

    assert!(config.prompt.include_workspace_summary);
    assert!(config.prompt.include_recent_messages);

    assert!(config.memory.enable_summary);
    assert!(config.memory.long_term_fact_extraction_enabled);
}

/// Tests that config can be cloned and remains independent.
#[test]
fn test_config_clone() {
    let original = AgentKernelConfig::default();
    let cloned = original.clone();

    assert_eq!(original.max_iters, cloned.max_iters);
    assert_eq!(original.planner_mode, cloned.planner_mode);
    assert_eq!(
        original.allow_request_planner_override,
        cloned.allow_request_planner_override
    );
}
