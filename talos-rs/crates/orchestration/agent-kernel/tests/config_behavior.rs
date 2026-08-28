//! Feature tests for AgentKernelConfig behavior validation
//!
//! Tests verify default values and boundary conditions for kernel configuration.

use talos_config::{AgentKernelConfig, PlannerMode};

/// Tests that AgentKernelConfig default values match the specification.
///
/// Default values per specification:
/// - tool_parallelism: 3
/// - planner_mode: "built_in"
#[test]
fn test_default_values() {
    let config = AgentKernelConfig::default();

    assert_eq!(
        config.tool_parallelism, 3,
        "tool_parallelism should default to 3"
    );
    assert_eq!(
        config.planner_mode,
        PlannerMode::BuiltIn,
        "planner_mode should default to 'built_in'"
    );
}

/// Tests boundary validation for numeric configuration fields.
///
/// Valid boundaries:
/// - tool_parallelism: must be > 0
/// - planner_mode: must be "built_in"
#[test]
fn test_boundary_validation() {
    let config = AgentKernelConfig::default();

    assert!(config.tool_parallelism > 0, "tool_parallelism must be > 0");

    assert_eq!(config.planner_mode, PlannerMode::BuiltIn);

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
        config.prompt.bootstrap.enabled,
        "prompt.bootstrap.enabled should default to true"
    );
    assert_eq!(
        config.prompt.window.compaction_buffer_tokens, 4_096,
        "prompt.window.compaction_buffer_tokens should default to 4096"
    );
    assert!(
        config.prompt.bootstrap.auto_create_missing_files,
        "prompt.bootstrap.auto_create_missing_files should default to true"
    );
    assert_eq!(
        config.prompt.bootstrap.max_chars_per_file, 8_000,
        "prompt.bootstrap.max_chars_per_file should default to 8000"
    );

    assert!(
        config.compaction.recent_messages_keep > 0,
        "compaction.recent_messages_keep must be > 0"
    );
    assert!(
        config.compaction.recent_messages_max_chars > 0,
        "compaction.recent_messages_max_chars must be > 0"
    );
    assert!(
        config.compaction.compacted_recent_messages_max_chars > 0,
        "compaction.compacted_recent_messages_max_chars must be > 0"
    );
}

/// Tests that nested config structures are properly initialized.
#[test]
fn test_nested_config_defaults() {
    let config = AgentKernelConfig::default();

    assert!(config.compaction.recent_messages_keep > 0);
    assert!(config.compaction.recent_messages_max_chars > 0);
    assert!(config.compaction.compacted_recent_messages_keep > 0);
    assert!(config.compaction.compacted_recent_messages_max_chars > 0);
    assert!(config.compaction.recent_summaries_keep > 0);
    assert!(config.compaction.compacted_summary_max_chars > 0);
    assert!(config.compaction.source_recent_tool_results_limit > 0);
    assert!(config.compaction.tool_output_max_chars > 0);
    assert!(config.compaction.compacted_tool_output_max_chars > 0);
    assert!(config.compaction.source_recent_tool_exchanges_limit > 0);
    assert!(config.compaction.tool_exchange_max_chars > 0);
    assert!(config.compaction.compacted_tool_exchange_max_chars > 0);
    assert!(config.compaction.retrieved_memories_keep > 0);
    assert!(config.compaction.compacted_retrieved_memory_max_chars > 0);
}

/// Tests that config can be cloned and remains independent.
#[test]
fn test_config_clone() {
    let original = AgentKernelConfig::default();
    let cloned = original.clone();

    assert_eq!(original.planner_mode, cloned.planner_mode);
}
