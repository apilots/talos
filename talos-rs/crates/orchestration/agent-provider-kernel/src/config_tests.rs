use super::*;

#[test]
fn normalization_applies_kernel_execution_minimums_once() {
    let config = AgentKernelConfig {
        tool_parallelism: 0,
        tool_result_max_bytes: 1,
        context_compaction_max_bytes: 1,
        ..AgentKernelConfig::default()
    };

    let normalized = normalize_kernel_config(config);

    assert_eq!(
        (
            normalized.tool_parallelism,
            normalized.tool_result_max_bytes,
            normalized.context_compaction_max_bytes,
        ),
        (1, 1024, 8 * 1024)
    );
}
