use serde::{Deserialize, Serialize};

use crate::{
    default_timezone, AgentKernelMcpConfig, AgentKernelMemoryConfig, AgentKernelPromptConfig,
    ExecutionMode, PlanReActPolicy, PlannerMode, PlannerSelection, ToolsRuntimeConfig,
};

/// Agent kernel runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelConfig {
    /// Maximum loop iterations in a single turn
    #[serde(default = "default_kernel_max_iters")]
    pub max_iters: u32,

    /// Maximum tasks allowed in a single turn
    #[serde(default = "default_kernel_max_task_per_turn")]
    pub max_task_per_turn: usize,

    /// Maximum parallel tool executions
    #[serde(default = "default_kernel_tool_parallelism")]
    pub tool_parallelism: usize,

    /// Maximum LLM calls in a turn
    #[serde(default = "default_kernel_max_llm_calls")]
    pub max_llm_calls: u32,

    /// Maximum tool-calling rounds in a general turn
    #[serde(default = "default_kernel_max_tool_rounds")]
    pub max_tool_rounds: u32,

    /// Maximum bytes returned by each tool execution payload
    #[serde(default = "default_kernel_tool_result_max_bytes")]
    pub tool_result_max_bytes: usize,

    /// Trigger context compaction when session memory exceeds this threshold
    #[serde(default = "default_kernel_context_compaction_max_bytes")]
    pub context_compaction_max_bytes: usize,

    /// Runtime timezone propagated from top-level config.
    #[serde(default = "default_timezone")]
    pub timezone: String,

    /// Default workspace id when request does not provide one
    #[serde(default = "default_workspace_id")]
    pub default_workspace_id: String,

    /// Planner mode used directly (Static) or as fallback (Auto).
    #[serde(default = "default_kernel_planner_mode")]
    pub planner_mode: PlannerMode,

    /// Planner selection strategy: Static uses planner_mode directly; Auto uses LLM inference.
    #[serde(default)]
    pub planner_selection: PlannerSelection,

    /// Kernel execution mode: single, sequential, parallel, or loop.
    #[serde(default = "default_kernel_execution_mode")]
    pub execution_mode: ExecutionMode,

    /// Child agent count used by sequential/parallel execution mode.
    #[serde(default = "default_kernel_execution_children")]
    pub execution_children: usize,

    /// Iteration count used by loop execution mode.
    #[serde(default = "default_kernel_execution_loop_iters")]
    pub execution_loop_iters: u32,

    /// Prompt builder defaults
    #[serde(default)]
    pub prompt: AgentKernelPromptConfig,

    /// Memory store defaults
    #[serde(default)]
    pub memory: AgentKernelMemoryConfig,

    /// Kernel agent binding defaults
    #[serde(default)]
    pub binding: AgentKernelBindingConfig,

    /// Tools runtime defaults
    #[serde(default)]
    pub tools: ToolsRuntimeConfig,

    /// MCP server connections available to this kernel
    #[serde(default)]
    pub mcp: AgentKernelMcpConfig,

    /// Loop control policy for PlanReAct mode.
    #[serde(default)]
    pub plan_react: PlanReActPolicy,
}

fn default_kernel_max_iters() -> u32 {
    1
}

fn default_kernel_max_task_per_turn() -> usize {
    3
}

fn default_kernel_tool_parallelism() -> usize {
    1
}

fn default_kernel_max_llm_calls() -> u32 {
    8
}

fn default_kernel_max_tool_rounds() -> u32 {
    8
}

fn default_kernel_tool_result_max_bytes() -> usize {
    64 * 1024
}

fn default_kernel_context_compaction_max_bytes() -> usize {
    256 * 1024
}

fn default_workspace_id() -> String {
    "default".to_string()
}

fn default_kernel_planner_mode() -> PlannerMode {
    PlannerMode::BuiltIn
}

fn default_kernel_execution_mode() -> ExecutionMode {
    ExecutionMode::Single
}

fn default_kernel_execution_children() -> usize {
    2
}

fn default_kernel_execution_loop_iters() -> u32 {
    3
}

/// Binding defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelBindingConfig {
    /// Default kernel agent kind
    #[serde(default = "default_kernel_binding_agent_kind")]
    pub default_agent_kind: String,
}

fn default_kernel_binding_agent_kind() -> String {
    "llm".to_string()
}

impl Default for AgentKernelBindingConfig {
    fn default() -> Self {
        Self {
            default_agent_kind: default_kernel_binding_agent_kind(),
        }
    }
}

impl Default for AgentKernelConfig {
    fn default() -> Self {
        Self {
            max_iters: default_kernel_max_iters(),
            max_task_per_turn: default_kernel_max_task_per_turn(),
            tool_parallelism: default_kernel_tool_parallelism(),
            max_llm_calls: default_kernel_max_llm_calls(),
            max_tool_rounds: default_kernel_max_tool_rounds(),
            tool_result_max_bytes: default_kernel_tool_result_max_bytes(),
            context_compaction_max_bytes: default_kernel_context_compaction_max_bytes(),
            timezone: default_timezone(),
            default_workspace_id: default_workspace_id(),
            planner_mode: default_kernel_planner_mode(),
            planner_selection: PlannerSelection::Static,
            execution_mode: default_kernel_execution_mode(),
            execution_children: default_kernel_execution_children(),
            execution_loop_iters: default_kernel_execution_loop_iters(),
            prompt: AgentKernelPromptConfig::default(),
            memory: AgentKernelMemoryConfig::default(),
            binding: AgentKernelBindingConfig::default(),
            tools: ToolsRuntimeConfig::default(),
            mcp: AgentKernelMcpConfig::default(),
            plan_react: PlanReActPolicy::default(),
        }
    }
}
