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

/// Prompt defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelPromptConfig {
    /// Static prompt token budget
    #[serde(default = "default_prompt_max_static_tokens")]
    pub max_static_tokens: usize,

    /// Dynamic prompt token budget
    #[serde(default = "default_prompt_max_dynamic_tokens")]
    pub max_dynamic_tokens: usize,

    /// Include session summary in prompt context
    #[serde(default = "default_true")]
    pub include_session_summary: bool,

    /// Include recent messages in prompt context
    #[serde(default = "default_true")]
    pub include_recent_messages: bool,

    /// Include retrieved memories in prompt context
    #[serde(default = "default_true")]
    pub include_retrieved_memories: bool,

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

fn default_prompt_max_static_tokens() -> usize {
    1200
}

fn default_prompt_max_dynamic_tokens() -> usize {
    3200
}

impl Default for AgentKernelPromptConfig {
    fn default() -> Self {
        Self {
            max_static_tokens: default_prompt_max_static_tokens(),
            max_dynamic_tokens: default_prompt_max_dynamic_tokens(),
            include_session_summary: true,
            include_recent_messages: true,
            include_retrieved_memories: true,
            retrieved_memory_max_items: default_prompt_retrieved_memory_max_items(),
            retrieved_memory_max_chars: default_prompt_retrieved_memory_max_chars(),
            bootstrap: AgentKernelBootstrapPromptConfig::default(),
        }
    }
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
