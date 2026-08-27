use talos_config::AgentKernelConfig;
use talos_prompt::{CompactPromptCursor, PromptLoadPolicy};

use super::types::SessionCompactionState;

fn build_prompt_load_policy(kernel_config: &AgentKernelConfig) -> PromptLoadPolicy {
    PromptLoadPolicy {
        compact_cursor: None,
        include_session_summary: true,
        include_recent_messages: true,
        include_recent_tool_history: true,
        include_recent_tool_outputs: true,
        session_recent_limit: kernel_config.compaction.recent_messages_keep,
        session_recent_max_chars: kernel_config.compaction.recent_messages_max_chars,
        recent_tool_history_limit: kernel_config.compaction.source_recent_tool_exchanges_limit,
        recent_tool_history_max_chars: kernel_config.compaction.tool_exchange_max_chars,
        recent_tool_outputs_limit: kernel_config.compaction.source_recent_tool_results_limit,
        recent_tool_outputs_max_chars: kernel_config.compaction.tool_output_max_chars,
        session_summary_recent_count: kernel_config.compaction.recent_summaries_keep,
        session_summary_max_chars: kernel_config.compaction.summary_max_chars,
        unified_recent_limit: 10,
    }
}

pub(super) fn build_initial_prompt_load_policy(
    kernel_config: &AgentKernelConfig,
    compaction_state: Option<&SessionCompactionState>,
) -> PromptLoadPolicy {
    if compaction_state.is_some_and(SessionCompactionState::is_recovery) {
        build_compacted_prompt_load_policy(kernel_config, None)
    } else {
        build_prompt_load_policy(kernel_config)
    }
}

pub(super) fn build_compacted_prompt_load_policy(
    kernel_config: &AgentKernelConfig,
    compact_cursor: Option<CompactPromptCursor>,
) -> PromptLoadPolicy {
    PromptLoadPolicy {
        compact_cursor,
        include_session_summary: true,
        include_recent_messages: true,
        include_recent_tool_history: true,
        include_recent_tool_outputs: true,
        session_recent_limit: kernel_config
            .compaction
            .compacted_recent_messages_keep
            .max(1),
        session_recent_max_chars: kernel_config.compaction.compacted_recent_messages_max_chars,
        recent_tool_history_limit: kernel_config.compaction.tool_exchange_keep_last.max(1),
        recent_tool_history_max_chars: kernel_config.compaction.compacted_tool_exchange_max_chars,
        recent_tool_outputs_limit: kernel_config.compaction.tool_output_keep_last.max(1),
        recent_tool_outputs_max_chars: kernel_config.compaction.compacted_tool_output_max_chars,
        session_summary_recent_count: kernel_config.compaction.recent_summaries_keep.max(1),
        session_summary_max_chars: kernel_config.compaction.compacted_summary_max_chars,
        unified_recent_limit: 10,
    }
}
