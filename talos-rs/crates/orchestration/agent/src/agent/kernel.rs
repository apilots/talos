use super::AgentHarness;
use std::sync::Arc;
use talos_agent_kernel::{StopReason, TaskKind, TaskResult, TaskStatus};
use talos_config::AgentKernelConfig;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_types::talos::{StopReason as TalosStopReason, TalosOutputItem, TalosUsage};

impl AgentHarness {
    pub(crate) fn normalized_kernel_config(
        mut kernel_config: AgentKernelConfig,
    ) -> AgentKernelConfig {
        if kernel_config.memory.workspace_dir.trim().is_empty() {
            kernel_config.memory.workspace_dir = AgentKernelConfig::default().memory.workspace_dir;
        }

        kernel_config.tool_parallelism = kernel_config.tool_parallelism.max(1);
        kernel_config.max_tool_rounds = kernel_config.max_tool_rounds.max(1);
        kernel_config.tool_result_max_bytes = kernel_config.tool_result_max_bytes.max(1024);
        kernel_config.context_compaction_max_bytes =
            kernel_config.context_compaction_max_bytes.max(8 * 1024);
        kernel_config.memory.hot_recent_messages = kernel_config.memory.hot_recent_messages.max(1);
        kernel_config.compaction.recent_messages_keep =
            kernel_config.compaction.recent_messages_keep.max(1);
        kernel_config.compaction.recent_messages_max_chars =
            kernel_config.compaction.recent_messages_max_chars.max(128);
        kernel_config.compaction.compacted_recent_messages_keep = kernel_config
            .compaction
            .compacted_recent_messages_keep
            .max(1);
        kernel_config.compaction.compacted_recent_messages_max_chars = kernel_config
            .compaction
            .compacted_recent_messages_max_chars
            .max(128)
            .min(kernel_config.compaction.recent_messages_max_chars);
        kernel_config.compaction.recent_summaries_keep =
            kernel_config.compaction.recent_summaries_keep.max(1);
        kernel_config.compaction.summary_max_chars =
            kernel_config.compaction.summary_max_chars.max(128);
        kernel_config.compaction.compacted_summary_max_chars = kernel_config
            .compaction
            .compacted_summary_max_chars
            .max(128)
            .min(kernel_config.compaction.summary_max_chars);
        kernel_config.compaction.source_recent_messages_limit =
            kernel_config.compaction.source_recent_messages_limit.max(1);
        kernel_config.compaction.source_recent_tool_results_limit = kernel_config
            .compaction
            .source_recent_tool_results_limit
            .max(1);
        kernel_config.compaction.tool_output_max_chars =
            kernel_config.compaction.tool_output_max_chars.max(128);
        kernel_config.compaction.compacted_tool_output_max_chars = kernel_config
            .compaction
            .compacted_tool_output_max_chars
            .max(128)
            .min(kernel_config.compaction.tool_output_max_chars);
        kernel_config.compaction.source_recent_tool_exchanges_limit = kernel_config
            .compaction
            .source_recent_tool_exchanges_limit
            .max(1);
        kernel_config.compaction.tool_exchange_max_chars =
            kernel_config.compaction.tool_exchange_max_chars.max(64);
        kernel_config.compaction.compacted_tool_exchange_max_chars = kernel_config
            .compaction
            .compacted_tool_exchange_max_chars
            .max(64)
            .min(kernel_config.compaction.tool_exchange_max_chars);
        kernel_config.compaction.retrieved_memories_keep =
            kernel_config.compaction.retrieved_memories_keep.max(1);
        kernel_config.compaction.retrieved_memory_max_chars =
            kernel_config.compaction.retrieved_memory_max_chars.max(128);
        kernel_config
            .compaction
            .compacted_retrieved_memory_max_chars = kernel_config
            .compaction
            .compacted_retrieved_memory_max_chars
            .max(128)
            .min(kernel_config.compaction.retrieved_memory_max_chars);
        kernel_config.memory.compaction_threshold_bytes =
            kernel_config.memory.compaction_threshold_bytes.max(1024);
        kernel_config.memory.search_max_results = kernel_config.memory.search_max_results.max(1);
        kernel_config
    }

    pub(crate) fn build_memory_store_from_kernel_config(
        kernel_config: &AgentKernelConfig,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> MemoryStore {
        MemoryStore::from_config_with_llm(kernel_config, llm_provider)
    }

    pub(crate) fn rebuild_memory_store(&mut self) {
        self.memory_store = Arc::new(Self::build_memory_store_from_kernel_config(
            &self.kernel_config,
            self.llm_provider.clone(),
        ));
    }
}

pub(crate) fn map_stop_reason(stop_reason: StopReason) -> TalosStopReason {
    match stop_reason {
        StopReason::EndTurn => TalosStopReason::EndTurn,
        StopReason::ModelOutputLimit => TalosStopReason::ModelOutputLimit,
        StopReason::ToolRoundLimit => TalosStopReason::ToolRoundLimit,
        StopReason::WaitingDynamicInput => TalosStopReason::WaitingDynamicInput,
        StopReason::TransferUnresolved => TalosStopReason::TransferUnresolved,
        StopReason::BeforeModelCallbackStopped => TalosStopReason::BeforeModelCallbackStopped,
        StopReason::AfterModelCallbackStopped => TalosStopReason::AfterModelCallbackStopped,
    }
}

#[allow(dead_code)]
pub(super) fn build_mock_task_result() -> TaskResult {
    TaskResult {
        task_id: "mock-task".to_string(),
        kind: TaskKind::General,
        goal: "mock".to_string(),
        status: TaskStatus::Success,
        output: "Mock response".to_string(),
        output_items: vec![TalosOutputItem::OutputText {
            text: "Mock response".to_string(),
        }],
        usage: TalosUsage::default(),
        warnings: Vec::new(),
        tool_traces: Vec::new(),
        state_delta: std::collections::HashMap::new(),
        error: None,
    }
}
