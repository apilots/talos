use talos_config::AgentKernelConfig;

/// Clamp Kernel configuration to the minimum safe execution bounds.
///
/// The Kernel Provider owns this normalization because it owns the effective
/// Kernel request. Callers must not duplicate these execution constraints.
pub fn normalize_kernel_config(mut config: AgentKernelConfig) -> AgentKernelConfig {
    config.tool_parallelism = config.tool_parallelism.max(1);
    config.tool_result_max_bytes = config.tool_result_max_bytes.max(1024);
    config.context_compaction_max_bytes = config.context_compaction_max_bytes.max(8 * 1024);
    config.compaction.recent_messages_keep = config.compaction.recent_messages_keep.max(1);
    config.compaction.recent_messages_max_chars =
        config.compaction.recent_messages_max_chars.max(128);
    config.compaction.compacted_recent_messages_keep =
        config.compaction.compacted_recent_messages_keep.max(1);
    config.compaction.compacted_recent_messages_max_chars = config
        .compaction
        .compacted_recent_messages_max_chars
        .max(128)
        .min(config.compaction.recent_messages_max_chars);
    config.compaction.recent_summaries_keep = config.compaction.recent_summaries_keep.max(1);
    config.compaction.summary_max_chars = config.compaction.summary_max_chars.max(128);
    config.compaction.compacted_summary_max_chars = config
        .compaction
        .compacted_summary_max_chars
        .max(128)
        .min(config.compaction.summary_max_chars);
    config.compaction.source_recent_messages_limit =
        config.compaction.source_recent_messages_limit.max(1);
    config.compaction.source_recent_tool_results_limit =
        config.compaction.source_recent_tool_results_limit.max(1);
    config.compaction.tool_output_max_chars = config.compaction.tool_output_max_chars.max(128);
    config.compaction.compacted_tool_output_max_chars = config
        .compaction
        .compacted_tool_output_max_chars
        .max(128)
        .min(config.compaction.tool_output_max_chars);
    config.compaction.source_recent_tool_exchanges_limit =
        config.compaction.source_recent_tool_exchanges_limit.max(1);
    config.compaction.tool_exchange_max_chars = config.compaction.tool_exchange_max_chars.max(64);
    config.compaction.compacted_tool_exchange_max_chars = config
        .compaction
        .compacted_tool_exchange_max_chars
        .max(64)
        .min(config.compaction.tool_exchange_max_chars);
    config.compaction.retrieved_memories_keep = config.compaction.retrieved_memories_keep.max(1);
    config.compaction.retrieved_memory_max_chars =
        config.compaction.retrieved_memory_max_chars.max(128);
    config.compaction.compacted_retrieved_memory_max_chars = config
        .compaction
        .compacted_retrieved_memory_max_chars
        .max(128)
        .min(config.compaction.retrieved_memory_max_chars);
    config
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
