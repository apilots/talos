use serde::{Deserialize, Serialize};

/// Prompt defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapPromptWarningMode {
    Off,
    Once,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelBootstrapPromptConfig {
    /// Enable workspace bootstrap prompt injection.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Auto-create missing bootstrap files before reading them.
    #[serde(default = "default_true")]
    pub auto_create_missing_files: bool,

    /// Maximum characters injected per bootstrap file.
    #[serde(default = "default_prompt_bootstrap_max_chars_per_file")]
    pub max_chars_per_file: usize,

    /// Maximum total characters injected across bootstrap files.
    #[serde(default = "default_prompt_bootstrap_max_chars_total")]
    pub max_chars_total: usize,

    /// Whether truncation warnings should be surfaced in diagnostics.
    #[serde(default = "default_bootstrap_prompt_warning_mode")]
    pub warning_mode: BootstrapPromptWarningMode,

    #[serde(default = "default_true")]
    pub include_agents: bool,

    #[serde(default = "default_true")]
    pub include_soul: bool,

    #[serde(default = "default_true")]
    pub include_identity: bool,

    #[serde(default = "default_true")]
    pub include_user: bool,

    #[serde(default = "default_true")]
    pub include_heartbeat: bool,

    #[serde(default = "default_true")]
    pub include_memory: bool,
}

fn default_prompt_bootstrap_max_chars_per_file() -> usize {
    8_000
}

fn default_prompt_bootstrap_max_chars_total() -> usize {
    24_000
}

fn default_bootstrap_prompt_warning_mode() -> BootstrapPromptWarningMode {
    BootstrapPromptWarningMode::Once
}

impl Default for AgentKernelBootstrapPromptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_create_missing_files: true,
            max_chars_per_file: default_prompt_bootstrap_max_chars_per_file(),
            max_chars_total: default_prompt_bootstrap_max_chars_total(),
            warning_mode: default_bootstrap_prompt_warning_mode(),
            include_agents: true,
            include_soul: true,
            include_identity: true,
            include_user: true,
            include_heartbeat: true,
            include_memory: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReservedOutputPolicy {
    UseModelDefault,
    Fixed { tokens: usize },
    Ratio { numerator: u32, denominator: u32 },
}

impl Default for ReservedOutputPolicy {
    fn default() -> Self {
        Self::UseModelDefault
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PromptWindowPolicy {
    #[serde(default)]
    pub reserved_output_tokens: ReservedOutputPolicy,

    #[serde(default = "default_prompt_compaction_buffer_tokens")]
    pub compaction_buffer_tokens: usize,
}

fn default_prompt_compaction_buffer_tokens() -> usize {
    4_096
}

impl Default for PromptWindowPolicy {
    fn default() -> Self {
        Self {
            reserved_output_tokens: ReservedOutputPolicy::default(),
            compaction_buffer_tokens: default_prompt_compaction_buffer_tokens(),
        }
    }
}

/// Prompt defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelPromptConfig {
    /// Prompt context window governance policy.
    #[serde(default)]
    pub window: PromptWindowPolicy,

    /// Include session summary in prompt context
    #[serde(default = "default_true")]
    pub include_session_summary: bool,

    /// Include recent messages in prompt context
    #[serde(default = "default_true")]
    pub include_recent_messages: bool,

    /// Target recent message count when building session prompt context.
    #[serde(default = "default_prompt_session_recent_messages_limit")]
    pub session_recent_messages_limit: usize,

    /// Include retrieved memories in prompt context
    #[serde(default = "default_true")]
    pub include_retrieved_memories: bool,

    /// Number of latest summaries to inject.
    #[serde(default = "default_prompt_session_summary_recent_count")]
    pub session_summary_recent_count: usize,

    /// Maximum characters retained per summary item.
    #[serde(default = "default_prompt_session_summary_max_chars")]
    pub session_summary_max_chars: usize,

    /// Maximum retrieved memory items injected into prompt
    #[serde(default = "default_prompt_retrieved_memory_max_items")]
    pub retrieved_memory_max_items: usize,

    /// Maximum total chars retained for retrieved memories
    #[serde(default = "default_prompt_retrieved_memory_max_chars")]
    pub retrieved_memory_max_chars: usize,

    /// Workspace bootstrap prompt policy.
    #[serde(default)]
    pub bootstrap: AgentKernelBootstrapPromptConfig,
}

impl Default for AgentKernelPromptConfig {
    fn default() -> Self {
        Self {
            window: PromptWindowPolicy::default(),
            include_session_summary: true,
            include_recent_messages: true,
            session_recent_messages_limit: default_prompt_session_recent_messages_limit(),
            include_retrieved_memories: true,
            session_summary_recent_count: default_prompt_session_summary_recent_count(),
            session_summary_max_chars: default_prompt_session_summary_max_chars(),
            retrieved_memory_max_items: default_prompt_retrieved_memory_max_items(),
            retrieved_memory_max_chars: default_prompt_retrieved_memory_max_chars(),
            bootstrap: AgentKernelBootstrapPromptConfig::default(),
        }
    }
}

fn default_prompt_session_recent_messages_limit() -> usize {
    24
}

fn default_prompt_session_summary_recent_count() -> usize {
    2
}

fn default_prompt_session_summary_max_chars() -> usize {
    1200
}

fn default_prompt_retrieved_memory_max_items() -> usize {
    4
}

fn default_prompt_retrieved_memory_max_chars() -> usize {
    2000
}

fn default_true() -> bool {
    true
}
