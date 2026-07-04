use super::AgentHarnessImpl;
use std::sync::Arc;
use talos_agent_kernel::{StopReason, TaskKind, TaskResult, TaskStatus};
use talos_config::{
    resolve_effective_kernel_workspace, validate_session_workspace_user, AgentKernelConfig,
};
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_types::talos::{RunItem, StopReason as TalosStopReason, TokenUsage};
use talos_types::TalosSession;

impl AgentHarnessImpl {
    pub(crate) fn normalized_kernel_config(kernel_config: AgentKernelConfig) -> AgentKernelConfig {
        normalized_kernel_config(kernel_config)
    }

    pub(crate) fn kernel_config_for_session(
        session: &TalosSession,
    ) -> TalosResult<AgentKernelConfig> {
        validate_session_workspace_user(&session.user_id, &session.context.user_id)?;
        let kernel_config = Self::normalized_kernel_config(session.config.agent.kernel.clone());
        Ok(resolve_effective_kernel_workspace(&kernel_config, &session.user_id)?.kernel_config)
    }

    pub(crate) async fn memory_store_for_session(
        &self,
        session: &TalosSession,
    ) -> TalosResult<(AgentKernelConfig, Arc<MemoryStore>)> {
        let kernel_config = Self::kernel_config_for_session(session)?;
        let llm_provider = self.llm_provider_for_session(session)?;
        let memory_store = Arc::new(Self::build_memory_store_from_kernel_config(
            &kernel_config,
            llm_provider,
        ));
        Ok((kernel_config, memory_store))
    }

    pub(crate) fn llm_provider_for_session(
        &self,
        session: &TalosSession,
    ) -> TalosResult<Arc<dyn LlmProvider>> {
        self.llm_provider_router
            .provider_for(&talos_types::ModelRef {
                provider: session.config.llm.provider.clone(),
                id: session.config.llm.model.clone(),
            })
    }

    pub(crate) fn build_memory_store_from_kernel_config(
        kernel_config: &AgentKernelConfig,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> MemoryStore {
        MemoryStore::from_config_with_llm(kernel_config, llm_provider)
    }
}

pub(crate) fn normalized_kernel_config(mut kernel_config: AgentKernelConfig) -> AgentKernelConfig {
    if kernel_config.workspace_dir.trim().is_empty() {
        kernel_config.workspace_dir = AgentKernelConfig::default().workspace_dir;
    }

    kernel_config.tool_parallelism = kernel_config.tool_parallelism.max(1);
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

pub(crate) fn map_stop_reason(stop_reason: StopReason) -> TalosStopReason {
    match stop_reason {
        StopReason::EndTurn => TalosStopReason::EndTurn,
        StopReason::ModelOutputLimit => TalosStopReason::ModelOutputLimit,
        StopReason::WaitingDynamicInput => TalosStopReason::WaitingDynamicInput,
        StopReason::TransferUnresolved => TalosStopReason::TransferUnresolved,
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
        output_items: vec![RunItem::OutputText {
            text: "Mock response".to_string(),
        }],
        usage: TokenUsage::default(),
        warnings: Vec::new(),
        tool_traces: Vec::new(),
        state_delta: std::collections::HashMap::new(),
        error: None,
    }
}
