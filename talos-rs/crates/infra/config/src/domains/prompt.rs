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
}

fn default_prompt_bootstrap_max_chars_per_file() -> usize {
    8_000
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
            warning_mode: default_bootstrap_prompt_warning_mode(),
            include_agents: true,
            include_soul: true,
            include_identity: true,
            include_user: true,
            include_heartbeat: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReservedOutputPolicy {
    #[default]
    UseModelDefault,
    Fixed {
        tokens: usize,
    },
    Ratio {
        numerator: u32,
        denominator: u32,
    },
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentKernelPromptConfig {
    /// Prompt context window governance policy.
    #[serde(default)]
    pub window: PromptWindowPolicy,

    /// Workspace bootstrap prompt policy.
    #[serde(default)]
    pub bootstrap: AgentKernelBootstrapPromptConfig,
}

fn default_true() -> bool {
    true
}
