use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use talos_config::{AgentKernelMemoryConfig, AgentKernelPromptConfig};
use talos_error::{TalosError, TalosResult};
use talos_memory::TalosMessageRole;

/// Prompt build phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptPhase {
    Planning,
    #[default]
    Execution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptMode {
    #[default]
    Full,
    Minimal,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannerMode {
    None,
    #[default]
    BuiltIn,
    PlanReAct,
}

impl PlannerMode {
    pub(crate) fn parse(value: &str) -> TalosResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "built_in" => Ok(Self::BuiltIn),
            "plan_react" => Ok(Self::PlanReAct),
            other => Err(TalosError::ValidationError(format!(
                "invalid planner mode: {}",
                other
            ))),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::BuiltIn => "built_in",
            Self::PlanReAct => "plan_react",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceBootstrapFileKind {
    Agents,
    Soul,
    Tools,
    Identity,
    User,
    Heartbeat,
    Bootstrap,
    Memory,
}

impl WorkspaceBootstrapFileKind {
    pub(crate) fn all() -> &'static [Self] {
        &[
            Self::Agents,
            Self::Soul,
            Self::Tools,
            Self::Identity,
            Self::User,
            Self::Heartbeat,
            Self::Bootstrap,
            Self::Memory,
        ]
    }

    pub(crate) fn file_name(self) -> &'static str {
        match self {
            Self::Agents => "AGENTS.md",
            Self::Soul => "SOUL.md",
            Self::Tools => "TOOLS.md",
            Self::Identity => "IDENTITY.md",
            Self::User => "USER.md",
            Self::Heartbeat => "HEARTBEAT.md",
            Self::Bootstrap => "BOOTSTRAP.md",
            Self::Memory => "MEMORY.md",
        }
    }

    pub(crate) fn default_template(self) -> &'static str {
        match self {
            Self::Agents => {
                "# AGENTS\n\nUse this file for workspace collaboration rules.\n\n## Rules\n- Add stable implementation rules here.\n"
            }
            Self::Soul => {
                "# SOUL\n\nUse this file for persona, tone, and style.\n\n## Style\n- Add voice and tone guidance here.\n"
            }
            Self::Tools => {
                "# TOOLS\n\nUse this file for local tool usage guidance.\n\n## Guidance\n- Add tool-specific operating notes here.\n"
            }
            Self::Identity => {
                "# IDENTITY\n\nUse this file for project or agent identity.\n\n## Identity\n- Add mission and identity notes here.\n"
            }
            Self::User => {
                "# USER\n\nUse this file for long-term user preferences.\n\n## Preferences\n- Add communication preferences here.\n"
            }
            Self::Heartbeat => {
                "# HEARTBEAT\n\nUse this file for heartbeat expectations.\n\n## Policy\n- Add heartbeat acknowledgement rules here.\n"
            }
            Self::Bootstrap => {
                "# BOOTSTRAP\n\nUse this file for first-run notes and workspace initialization.\n\n## Checklist\n- Add setup tasks here.\n"
            }
            Self::Memory => {
                "# MEMORY\n\nUse this file for durable user-written memory.\n\n## Preferences\n- Add long-term user preferences here.\n\n## Constraints\n- Add important constraints and rules here.\n\n## Ongoing Goals\n- Track current long-running goals here.\n\n## Notes\n- Keep stable facts and project context here.\n"
            }
        }
    }

    pub(crate) fn should_load_in_mode(
        self,
        mode: PromptMode,
        prompt_config: &AgentKernelPromptConfig,
    ) -> bool {
        let include_by_kind = match self {
            Self::Agents => prompt_config.bootstrap.include_agents,
            Self::Soul => prompt_config.bootstrap.include_soul,
            Self::Tools => prompt_config.bootstrap.include_tools,
            Self::Identity => prompt_config.bootstrap.include_identity,
            Self::User => prompt_config.bootstrap.include_user,
            Self::Heartbeat => prompt_config.bootstrap.include_heartbeat,
            Self::Bootstrap => prompt_config.bootstrap.include_bootstrap,
            Self::Memory => prompt_config.bootstrap.include_memory,
        };
        if !include_by_kind {
            return false;
        }
        match mode {
            PromptMode::Full => true,
            PromptMode::Minimal => matches!(
                self,
                Self::Agents | Self::Soul | Self::Tools | Self::Identity | Self::User
            ),
            PromptMode::None => false,
        }
    }

    pub(crate) fn is_project_context(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBootstrapFile {
    pub kind: WorkspaceBootstrapFileKind,
    pub path: PathBuf,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionKind {
    Identity,
    PlanningInstruction,
    ProjectContext,
    SessionSummary,
    RecentMessages,
    RetrievedMemories,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetTier {
    Critical,
    Important,
    Supplemental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationMode {
    DropWhole,
    TrimTail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSection {
    pub kind: PromptSectionKind,
    pub title: Option<String>,
    pub content: String,
    pub role: Option<TalosMessageRole>,
    pub tier: BudgetTier,
    pub required: bool,
    pub estimated_tokens: usize,
    pub truncation_mode: TruncationMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapFileDiagnostic {
    pub kind: WorkspaceBootstrapFileKind,
    pub path: PathBuf,
    pub raw_chars: usize,
    pub injected_chars: usize,
    pub truncated: bool,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDiagnostics {
    pub mode: PromptMode,
    pub included_sections: Vec<PromptSectionKind>,
    pub dropped_sections: Vec<PromptSectionKind>,
    pub truncated_sections: Vec<PromptSectionKind>,
    pub bootstrap_files: Vec<BootstrapFileDiagnostic>,
    pub total_estimated_tokens: usize,
    pub static_estimated_tokens: usize,
    pub dynamic_estimated_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAssembly {
    pub system_sections: Vec<PromptSection>,
    pub context_sections: Vec<PromptSection>,
    pub user_input: String,
    pub diagnostics: PromptDiagnostics,
}

/// Prompt budget controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptBudget {
    pub max_static_tokens: usize,
    pub max_dynamic_tokens: usize,
}

impl Default for PromptBudget {
    fn default() -> Self {
        Self {
            max_static_tokens: 1200,
            max_dynamic_tokens: 3200,
        }
    }
}

/// Prompt context loading policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptLoadPolicy {
    pub include_session_summary: bool,
    pub include_recent_messages: bool,
    pub include_retrieved_memories: bool,
    pub session_recent_limit: usize,
    pub retrieved_memory_max_items: usize,
    pub retrieved_memory_max_chars: usize,
    pub unified_recent_limit: usize,
}

impl Default for PromptLoadPolicy {
    fn default() -> Self {
        Self {
            include_session_summary: true,
            include_recent_messages: true,
            include_retrieved_memories: true,
            session_recent_limit: 16,
            retrieved_memory_max_items: 4,
            retrieved_memory_max_chars: 2000,
            unified_recent_limit: 10,
        }
    }
}

/// Prompt build configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptBuildConfig {
    pub budget: PromptBudget,
    pub load_policy: PromptLoadPolicy,
    pub prompt: AgentKernelPromptConfig,
    pub memory: AgentKernelMemoryConfig,
}
