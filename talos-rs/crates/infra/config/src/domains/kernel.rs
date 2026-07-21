use serde::{Deserialize, Serialize};

use crate::{
    default_timezone, AgentKernelMcpConfig, AgentKernelPromptConfig, PlannerMode,
    PluginsRuntimeConfig, SkillsRuntimeConfig, ToolsRuntimeConfig,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelCompactionConfig {
    #[serde(default)]
    pub model: Option<String>,

    #[serde(default = "default_kernel_compaction_trigger_ratio_percent")]
    pub trigger_ratio_percent: u8,

    #[serde(default = "default_kernel_compaction_hard_ratio_percent")]
    pub hard_ratio_percent: u8,

    #[serde(default = "default_kernel_compaction_recent_messages_keep")]
    pub recent_messages_keep: usize,

    #[serde(default = "default_kernel_compaction_recent_messages_max_chars")]
    pub recent_messages_max_chars: usize,

    #[serde(default = "default_kernel_compaction_compacted_recent_messages_keep")]
    pub compacted_recent_messages_keep: usize,

    #[serde(default = "default_kernel_compaction_compacted_recent_messages_max_chars")]
    pub compacted_recent_messages_max_chars: usize,

    #[serde(default = "default_kernel_compaction_recent_summaries_keep")]
    pub recent_summaries_keep: usize,

    #[serde(default = "default_kernel_compaction_summary_max_chars")]
    pub summary_max_chars: usize,

    #[serde(default = "default_kernel_compaction_compacted_summary_max_chars")]
    pub compacted_summary_max_chars: usize,

    #[serde(default = "default_kernel_compaction_source_recent_messages_limit")]
    pub source_recent_messages_limit: usize,

    #[serde(default = "default_kernel_compaction_source_recent_tool_results_limit")]
    pub source_recent_tool_results_limit: usize,

    #[serde(default = "default_kernel_compaction_tool_output_keep_last")]
    pub tool_output_keep_last: usize,

    #[serde(default = "default_kernel_compaction_tool_output_max_chars")]
    pub tool_output_max_chars: usize,

    #[serde(default = "default_kernel_compaction_compacted_tool_output_max_chars")]
    pub compacted_tool_output_max_chars: usize,

    #[serde(default = "default_kernel_compaction_source_recent_tool_exchanges_limit")]
    pub source_recent_tool_exchanges_limit: usize,

    #[serde(default = "default_kernel_compaction_tool_exchange_keep_last")]
    pub tool_exchange_keep_last: usize,

    #[serde(default = "default_kernel_compaction_tool_exchange_max_chars")]
    pub tool_exchange_max_chars: usize,

    #[serde(default = "default_kernel_compaction_compacted_tool_exchange_max_chars")]
    pub compacted_tool_exchange_max_chars: usize,

    #[serde(default = "default_kernel_compaction_retrieved_memories_keep")]
    pub retrieved_memories_keep: usize,

    #[serde(default = "default_kernel_compaction_retrieved_memory_max_chars")]
    pub retrieved_memory_max_chars: usize,

    #[serde(default = "default_kernel_compaction_compacted_retrieved_memory_max_chars")]
    pub compacted_retrieved_memory_max_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelTimelineEventsConfig {
    #[serde(default = "default_timeline_events_retention_hours")]
    pub retention_hours: u32,

    #[serde(default = "default_timeline_events_max_events_per_session")]
    pub max_events_per_session: usize,

    #[serde(default = "default_timeline_events_gc_interval_minutes")]
    pub gc_interval_minutes: u32,

    #[serde(default = "default_timeline_events_gc_batch_size")]
    pub gc_batch_size: usize,

    #[serde(default = "default_timeline_events_replay_batch_size")]
    pub replay_batch_size: usize,

    #[serde(default = "default_timeline_events_first_event_timeout_ms")]
    pub first_event_timeout_ms: u64,

    #[serde(default = "default_timeline_events_idempotency_key_ttl_hours")]
    pub idempotency_key_ttl_hours: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelOutputConfig {
    #[serde(default = "default_kernel_output_max_tokens")]
    pub max_tokens: u32,

    #[serde(default = "default_kernel_output_upper_limit")]
    pub upper_limit: u32,
}

impl AgentKernelOutputConfig {
    pub fn effective_max_tokens(&self) -> u32 {
        self.max_tokens.min(self.upper_limit).max(1)
    }
}

fn default_kernel_output_max_tokens() -> u32 {
    32_000
}

fn default_kernel_output_upper_limit() -> u32 {
    64_000
}

impl Default for AgentKernelOutputConfig {
    fn default() -> Self {
        Self {
            max_tokens: default_kernel_output_max_tokens(),
            upper_limit: default_kernel_output_upper_limit(),
        }
    }
}

fn default_timeline_events_retention_hours() -> u32 {
    24
}

fn default_timeline_events_max_events_per_session() -> usize {
    10_000
}

fn default_timeline_events_gc_interval_minutes() -> u32 {
    30
}

fn default_timeline_events_gc_batch_size() -> usize {
    1_000
}

fn default_timeline_events_replay_batch_size() -> usize {
    1_024
}

fn default_timeline_events_first_event_timeout_ms() -> u64 {
    5_000
}

fn default_timeline_events_idempotency_key_ttl_hours() -> u32 {
    24
}

impl Default for AgentKernelTimelineEventsConfig {
    fn default() -> Self {
        Self {
            retention_hours: default_timeline_events_retention_hours(),
            max_events_per_session: default_timeline_events_max_events_per_session(),
            gc_interval_minutes: default_timeline_events_gc_interval_minutes(),
            gc_batch_size: default_timeline_events_gc_batch_size(),
            replay_batch_size: default_timeline_events_replay_batch_size(),
            first_event_timeout_ms: default_timeline_events_first_event_timeout_ms(),
            idempotency_key_ttl_hours: default_timeline_events_idempotency_key_ttl_hours(),
        }
    }
}

fn default_kernel_compaction_trigger_ratio_percent() -> u8 {
    85
}

fn default_kernel_compaction_hard_ratio_percent() -> u8 {
    95
}

fn default_kernel_compaction_recent_messages_keep() -> usize {
    24
}

fn default_kernel_compaction_recent_messages_max_chars() -> usize {
    4_000
}

fn default_kernel_compaction_compacted_recent_messages_keep() -> usize {
    2
}

fn default_kernel_compaction_compacted_recent_messages_max_chars() -> usize {
    1_200
}

fn default_kernel_compaction_recent_summaries_keep() -> usize {
    2
}

fn default_kernel_compaction_summary_max_chars() -> usize {
    1_200
}

fn default_kernel_compaction_compacted_summary_max_chars() -> usize {
    800
}

fn default_kernel_compaction_source_recent_messages_limit() -> usize {
    48
}

fn default_kernel_compaction_source_recent_tool_results_limit() -> usize {
    12
}

fn default_kernel_compaction_tool_output_keep_last() -> usize {
    2
}

fn default_kernel_compaction_tool_output_max_chars() -> usize {
    2_000
}

fn default_kernel_compaction_compacted_tool_output_max_chars() -> usize {
    600
}

fn default_kernel_compaction_source_recent_tool_exchanges_limit() -> usize {
    12
}

fn default_kernel_compaction_tool_exchange_keep_last() -> usize {
    2
}

fn default_kernel_compaction_tool_exchange_max_chars() -> usize {
    600
}

fn default_kernel_compaction_compacted_tool_exchange_max_chars() -> usize {
    300
}

fn default_kernel_compaction_retrieved_memories_keep() -> usize {
    4
}

fn default_kernel_compaction_retrieved_memory_max_chars() -> usize {
    2_000
}

fn default_kernel_compaction_compacted_retrieved_memory_max_chars() -> usize {
    800
}

impl Default for AgentKernelCompactionConfig {
    fn default() -> Self {
        Self {
            model: None,
            trigger_ratio_percent: default_kernel_compaction_trigger_ratio_percent(),
            hard_ratio_percent: default_kernel_compaction_hard_ratio_percent(),
            recent_messages_keep: default_kernel_compaction_recent_messages_keep(),
            recent_messages_max_chars: default_kernel_compaction_recent_messages_max_chars(),
            compacted_recent_messages_keep:
                default_kernel_compaction_compacted_recent_messages_keep(),
            compacted_recent_messages_max_chars:
                default_kernel_compaction_compacted_recent_messages_max_chars(),
            recent_summaries_keep: default_kernel_compaction_recent_summaries_keep(),
            summary_max_chars: default_kernel_compaction_summary_max_chars(),
            compacted_summary_max_chars: default_kernel_compaction_compacted_summary_max_chars(),
            source_recent_messages_limit: default_kernel_compaction_source_recent_messages_limit(),
            source_recent_tool_results_limit:
                default_kernel_compaction_source_recent_tool_results_limit(),
            tool_output_keep_last: default_kernel_compaction_tool_output_keep_last(),
            tool_output_max_chars: default_kernel_compaction_tool_output_max_chars(),
            compacted_tool_output_max_chars:
                default_kernel_compaction_compacted_tool_output_max_chars(),
            source_recent_tool_exchanges_limit:
                default_kernel_compaction_source_recent_tool_exchanges_limit(),
            tool_exchange_keep_last: default_kernel_compaction_tool_exchange_keep_last(),
            tool_exchange_max_chars: default_kernel_compaction_tool_exchange_max_chars(),
            compacted_tool_exchange_max_chars:
                default_kernel_compaction_compacted_tool_exchange_max_chars(),
            retrieved_memories_keep: default_kernel_compaction_retrieved_memories_keep(),
            retrieved_memory_max_chars: default_kernel_compaction_retrieved_memory_max_chars(),
            compacted_retrieved_memory_max_chars:
                default_kernel_compaction_compacted_retrieved_memory_max_chars(),
        }
    }
}

/// Agent kernel runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelConfig {
    /// Effective agent workspace root for prompt, memory, sessions, and file tool fallback.
    #[serde(default = "default_kernel_workspace_dir")]
    pub workspace_dir: String,

    /// Maximum parallel tool executions
    #[serde(default = "default_kernel_tool_parallelism")]
    pub tool_parallelism: usize,

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

    /// Planner mode used by the general turn policy.
    #[serde(default = "default_kernel_planner_mode")]
    pub planner_mode: PlannerMode,

    /// Prompt builder defaults
    #[serde(default)]
    pub prompt: AgentKernelPromptConfig,

    /// LLM response output token policy.
    #[serde(default)]
    pub output: AgentKernelOutputConfig,

    /// Context compaction policy and model override.
    #[serde(default)]
    pub compaction: AgentKernelCompactionConfig,

    /// Timeline event replay and retention defaults.
    #[serde(default)]
    pub timeline_events: AgentKernelTimelineEventsConfig,

    /// Tools runtime defaults
    #[serde(default)]
    pub tools: ToolsRuntimeConfig,

    /// Skills runtime defaults
    #[serde(default)]
    pub skills: SkillsRuntimeConfig,

    /// Plugin manager runtime defaults
    #[serde(default)]
    pub plugins: PluginsRuntimeConfig,

    /// MCP server connections available to this kernel
    #[serde(default)]
    pub mcp: AgentKernelMcpConfig,
}

fn default_kernel_tool_parallelism() -> usize {
    1
}

fn default_kernel_workspace_dir() -> String {
    crate::TalosPaths::resolve()
        .workspace_dir_for_agent("talos")
        .to_string_lossy()
        .to_string()
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

impl Default for AgentKernelConfig {
    fn default() -> Self {
        Self {
            workspace_dir: default_kernel_workspace_dir(),
            tool_parallelism: default_kernel_tool_parallelism(),
            tool_result_max_bytes: default_kernel_tool_result_max_bytes(),
            context_compaction_max_bytes: default_kernel_context_compaction_max_bytes(),
            timezone: default_timezone(),
            default_workspace_id: default_workspace_id(),
            planner_mode: default_kernel_planner_mode(),
            prompt: AgentKernelPromptConfig::default(),
            output: AgentKernelOutputConfig::default(),
            compaction: AgentKernelCompactionConfig::default(),
            timeline_events: AgentKernelTimelineEventsConfig::default(),
            tools: ToolsRuntimeConfig::default(),
            skills: SkillsRuntimeConfig::default(),
            plugins: PluginsRuntimeConfig::default(),
            mcp: AgentKernelMcpConfig::default(),
        }
    }
}
