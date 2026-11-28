use crate::validate::is_unresolved_placeholder;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::{
    default_timezone, AgentKernelPromptConfig, PlannerMode, PluginsRuntimeConfig, RuntimeEnv,
    SkillsRuntimeConfig, ToolsRuntimeConfig,
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

/// Kernel Provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelConfig {
    /// Runtime environment loaded by the Talos configuration layer.
    #[serde(skip)]
    pub runtime_env: RuntimeEnv,

    /// Runtime-only effective workspace root injected for one execution.
    #[serde(skip)]
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

    /// Tools runtime defaults
    #[serde(default)]
    pub tools: ToolsRuntimeConfig,

    /// Skills runtime defaults
    #[serde(default)]
    pub skills: SkillsRuntimeConfig,

    /// Plugin manager runtime defaults
    #[serde(default)]
    pub plugins: PluginsRuntimeConfig,
}

fn default_kernel_tool_parallelism() -> usize {
    3
}

fn default_kernel_tool_result_max_bytes() -> usize {
    64 * 1024
}

fn default_kernel_context_compaction_max_bytes() -> usize {
    256 * 1024
}

fn default_kernel_planner_mode() -> PlannerMode {
    PlannerMode::BuiltIn
}

impl Default for AgentKernelConfig {
    fn default() -> Self {
        Self {
            runtime_env: RuntimeEnv::default(),
            workspace_dir: String::new(),
            tool_parallelism: default_kernel_tool_parallelism(),
            tool_result_max_bytes: default_kernel_tool_result_max_bytes(),
            context_compaction_max_bytes: default_kernel_context_compaction_max_bytes(),
            timezone: default_timezone(),
            planner_mode: default_kernel_planner_mode(),
            prompt: AgentKernelPromptConfig::default(),
            output: AgentKernelOutputConfig::default(),
            compaction: AgentKernelCompactionConfig::default(),
            tools: ToolsRuntimeConfig::default(),
            skills: SkillsRuntimeConfig::default(),
            plugins: PluginsRuntimeConfig::default(),
        }
    }
}

impl AgentKernelConfig {
    /// Validate shared structural policy without opening tools or external resources.
    pub(crate) fn validate(&self, path: &str) -> TalosResult<()> {
        crate::parse_timezone(&self.timezone)
            .map_err(|error| error.context(format!("validate {path}.timezone")))?;
        if self.tool_parallelism == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.tool_parallelism must be > 0"),
            ));
        }
        if self.tool_result_max_bytes == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.tool_result_max_bytes must be > 0"),
            ));
        }
        if self.context_compaction_max_bytes == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.context_compaction_max_bytes must be > 0"),
            ));
        }
        self.tools
            .environment
            .validate()
            .map_err(|error| error.context(format!("validate {path}.tools.environment")))?;
        let logs = &self.tools.bash.output_log;
        logs.validate()
            .map_err(|error| error.context(format!("validate {path}.tools.bash.output_log")))?;
        if self.tools.bash.max_output_bytes < 256 || self.tool_result_max_bytes < 256 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Tool result and local execution output limits must be at least 256 bytes.",
            ));
        }
        if self.prompt.bootstrap.max_chars_per_file == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.prompt.bootstrap.max_chars_per_file must be > 0"),
            ));
        }
        if self.prompt.window.compaction_buffer_tokens == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.prompt.window.compaction_buffer_tokens must be > 0"),
            ));
        }
        if self.compaction.trigger_ratio_percent == 0
            || self.compaction.trigger_ratio_percent >= 100
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.compaction.trigger_ratio_percent must be in 1..100"),
            ));
        }
        if self.compaction.hard_ratio_percent == 0 || self.compaction.hard_ratio_percent > 100 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.compaction.hard_ratio_percent must be in 1..=100"),
            ));
        }
        if self.compaction.trigger_ratio_percent >= self.compaction.hard_ratio_percent {
            return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest,
            format!("{path}.compaction.trigger_ratio_percent must be < {path}.compaction.hard_ratio_percent"),
        ));
        }
        for (name, value) in [
            ("recent_messages_keep", self.compaction.recent_messages_keep),
            (
                "recent_messages_max_chars",
                self.compaction.recent_messages_max_chars,
            ),
            (
                "compacted_recent_messages_max_chars",
                self.compaction.compacted_recent_messages_max_chars,
            ),
            (
                "compacted_recent_messages_keep",
                self.compaction.compacted_recent_messages_keep,
            ),
            (
                "recent_summaries_keep",
                self.compaction.recent_summaries_keep,
            ),
            ("summary_max_chars", self.compaction.summary_max_chars),
            (
                "compacted_summary_max_chars",
                self.compaction.compacted_summary_max_chars,
            ),
            (
                "source_recent_messages_limit",
                self.compaction.source_recent_messages_limit,
            ),
            (
                "source_recent_tool_results_limit",
                self.compaction.source_recent_tool_results_limit,
            ),
            (
                "tool_output_max_chars",
                self.compaction.tool_output_max_chars,
            ),
            (
                "compacted_tool_output_max_chars",
                self.compaction.compacted_tool_output_max_chars,
            ),
            (
                "source_recent_tool_exchanges_limit",
                self.compaction.source_recent_tool_exchanges_limit,
            ),
            (
                "tool_exchange_max_chars",
                self.compaction.tool_exchange_max_chars,
            ),
            (
                "compacted_tool_exchange_max_chars",
                self.compaction.compacted_tool_exchange_max_chars,
            ),
            (
                "retrieved_memories_keep",
                self.compaction.retrieved_memories_keep,
            ),
            (
                "retrieved_memory_max_chars",
                self.compaction.retrieved_memory_max_chars,
            ),
            (
                "compacted_retrieved_memory_max_chars",
                self.compaction.compacted_retrieved_memory_max_chars,
            ),
        ] {
            if value == 0 {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("{path}.compaction.{name} must be > 0"),
                ));
            }
        }
        if self.compaction.compacted_recent_messages_max_chars
            > self.compaction.recent_messages_max_chars
        {
            return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest,
            format!("{path}.compaction.compacted_recent_messages_max_chars must be <= {path}.compaction.recent_messages_max_chars"),
        ));
        }
        if self.compaction.compacted_summary_max_chars > self.compaction.summary_max_chars {
            return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest,
            format!("{path}.compaction.compacted_summary_max_chars must be <= {path}.compaction.summary_max_chars"),
        ));
        }
        if self.compaction.compacted_tool_output_max_chars > self.compaction.tool_output_max_chars {
            return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest,
            format!("{path}.compaction.compacted_tool_output_max_chars must be <= {path}.compaction.tool_output_max_chars"),
        ));
        }
        if self.compaction.compacted_tool_exchange_max_chars
            > self.compaction.tool_exchange_max_chars
        {
            return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest,
            format!("{path}.compaction.compacted_tool_exchange_max_chars must be <= {path}.compaction.tool_exchange_max_chars"),
        ));
        }
        if self.compaction.compacted_retrieved_memory_max_chars
            > self.compaction.retrieved_memory_max_chars
        {
            return Err(TalosError::new(talos_error::ErrorCode::InvalidRequest,
            format!("{path}.compaction.compacted_retrieved_memory_max_chars must be <= {path}.compaction.retrieved_memory_max_chars"),
        ));
        }
        if let Some(model) = self.compaction.model.as_ref() {
            if model.trim().is_empty() || is_unresolved_placeholder(model) {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("{path}.compaction.model is empty or unresolved placeholder"),
                ));
            }
        }
        if self.output.max_tokens == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.output.max_tokens must be > 0"),
            ));
        }
        if self.output.upper_limit == 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.output.upper_limit must be > 0"),
            ));
        }
        if self.output.max_tokens > self.output.upper_limit {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("{path}.output.max_tokens must be <= {path}.output.upper_limit"),
            ));
        }
        match &self.prompt.window.reserved_output_tokens {
            crate::ReservedOutputPolicy::UseModelDefault => {}
            crate::ReservedOutputPolicy::Fixed { tokens } => {
                if *tokens == 0 {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!(
                            "{path}.prompt.window.reserved_output_tokens.fixed.tokens must be > 0"
                        ),
                    ));
                }
            }
            crate::ReservedOutputPolicy::Ratio {
                numerator,
                denominator,
            } => {
                if *numerator == 0 || *denominator == 0 {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!("{path}.prompt.window.reserved_output_tokens.ratio must be > 0"),
                    ));
                }
            }
        }
        Ok(())
    }
}
