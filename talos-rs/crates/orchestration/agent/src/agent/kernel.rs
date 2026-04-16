use super::{AgentImpl, KernelExecutionMode};
use std::sync::Arc;
use talos_agent_kernel::{
    Agent as KernelAgent, LlmKernelAgent, LoopKernelAgent, ParallelKernelAgent, PlannerSelector,
    SequentialKernelAgent, StopReason, TaskKind, TaskResult, TaskStatus,
};
use talos_config::{AgentKernelConfig, ExecutionMode};
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_types::talos::{StopReason as TalosStopReason, TalosOutputItem, TalosUsage};

impl AgentImpl {
    pub(crate) fn normalized_kernel_config(
        mut kernel_config: AgentKernelConfig,
    ) -> AgentKernelConfig {
        if kernel_config.memory.workspace_dir.trim().is_empty() {
            kernel_config.memory.workspace_dir = AgentKernelConfig::default().memory.workspace_dir;
        }

        kernel_config.max_iters = kernel_config.max_iters.max(1);
        kernel_config.max_task_per_turn = kernel_config.max_task_per_turn.max(1);
        kernel_config.tool_parallelism = kernel_config.tool_parallelism.max(1);
        kernel_config.max_llm_calls = kernel_config.max_llm_calls.max(1);
        kernel_config.max_tool_rounds = kernel_config.max_tool_rounds.max(1);
        kernel_config.tool_result_max_bytes = kernel_config.tool_result_max_bytes.max(1024);
        kernel_config.context_compaction_max_bytes =
            kernel_config.context_compaction_max_bytes.max(8 * 1024);
        kernel_config.memory.hot_recent_messages = kernel_config.memory.hot_recent_messages.max(1);
        kernel_config.prompt.session_recent_messages_limit =
            kernel_config.prompt.session_recent_messages_limit.max(1);
        kernel_config.prompt.session_summary_recent_count =
            kernel_config.prompt.session_summary_recent_count.max(1);
        kernel_config.prompt.session_summary_max_chars =
            kernel_config.prompt.session_summary_max_chars.max(128);
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

    pub(crate) fn kernel_config_snapshot(&self) -> AgentKernelConfig {
        self.kernel_config.clone()
    }

    pub(crate) fn resolve_kernel_mode(&self) -> TalosResult<KernelExecutionMode> {
        let mode = self.kernel_config.execution_mode;
        Ok(match mode {
            ExecutionMode::Single => KernelExecutionMode::Single,
            ExecutionMode::Sequential => KernelExecutionMode::Sequential,
            ExecutionMode::Parallel => KernelExecutionMode::Parallel,
            ExecutionMode::Loop => KernelExecutionMode::Loop,
        })
    }

    pub(crate) fn build_llm_kernel_agent(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Arc<dyn KernelAgent> {
        let planner_selector = Arc::new(PlannerSelector::new(self.llm_provider.clone()));
        Arc::new(LlmKernelAgent::new(
            id,
            name,
            self.description.clone(),
            self.llm_provider.clone(),
            self.memory_store.clone(),
            self.kernel_config_snapshot(),
            self.tool_registry.clone(),
            self.tool_registry.clone(),
            planner_selector,
        ))
    }

    pub(crate) fn build_kernel_agent_for_request(&self) -> TalosResult<Arc<dyn KernelAgent>> {
        let mode = self.resolve_kernel_mode()?;
        let child_count = self.kernel_config.execution_children.clamp(1, 4);
        let loop_iters = self.kernel_config.execution_loop_iters.clamp(1, 16);

        Ok(match mode {
            KernelExecutionMode::Single => {
                self.build_llm_kernel_agent(self.id.clone(), self.name.clone())
            }
            KernelExecutionMode::Sequential => {
                let children = (0..child_count)
                    .map(|idx| {
                        self.build_llm_kernel_agent(
                            format!("{}-seq-{}", self.id, idx + 1),
                            format!("{} seq {}", self.name, idx + 1),
                        )
                    })
                    .collect::<Vec<_>>();
                Arc::new(SequentialKernelAgent::new(
                    format!("{}-sequential", self.id),
                    children,
                ))
            }
            KernelExecutionMode::Parallel => {
                let children = (0..child_count)
                    .map(|idx| {
                        self.build_llm_kernel_agent(
                            format!("{}-par-{}", self.id, idx + 1),
                            format!("{} par {}", self.name, idx + 1),
                        )
                    })
                    .collect::<Vec<_>>();
                Arc::new(ParallelKernelAgent::new(
                    format!("{}-parallel", self.id),
                    children,
                ))
            }
            KernelExecutionMode::Loop => Arc::new(LoopKernelAgent::new(
                format!("{}-loop", self.id),
                self.build_llm_kernel_agent(
                    format!("{}-loop-body", self.id),
                    format!("{} loop body", self.name),
                ),
                loop_iters,
            )),
        })
    }
}

pub(crate) fn map_stop_reason(stop_reason: StopReason) -> TalosStopReason {
    match stop_reason {
        StopReason::EndTurn => TalosStopReason::EndTurn,
        StopReason::ToolRoundLimit => TalosStopReason::ToolRoundLimit,
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
