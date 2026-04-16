use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use talos_config::{AgentKernelMemoryConfig, AgentKernelPromptConfig, ReservedOutputPolicy};
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
    Identity,
    User,
    Heartbeat,
    Memory,
}

impl WorkspaceBootstrapFileKind {
    pub(crate) fn workspace_files() -> &'static [Self] {
        &[
            Self::Soul,
            Self::Identity,
            Self::User,
            Self::Heartbeat,
            Self::Memory,
        ]
    }

    pub(crate) fn project_files() -> &'static [Self] {
        &[Self::Agents]
    }

    pub(crate) fn file_name(self) -> &'static str {
        match self {
            Self::Agents => "AGENTS.md",
            Self::Soul => "SOUL.md",
            Self::Identity => "IDENTITY.md",
            Self::User => "USER.md",
            Self::Heartbeat => "HEARTBEAT.md",
            Self::Memory => "MEMORY.md",
        }
    }

    pub(crate) fn default_template(self) -> &'static str {
        match self {
            Self::Agents => {
                "# AGENTS\n\nUse this file for repository-local collaboration rules.\n\n## Rules\n- Add durable implementation rules for this project.\n- Document repository-specific constraints, workflows, and review expectations.\n"
            }
            Self::Soul => {
                "# SOUL\n\nTalos is a long-running intelligent steward. Stay evidence-driven, proactive, and calm.\n\n## Behavior\n- Prefer verified facts over confident guesses.\n- Progress tasks with minimal user supervision.\n- Surface uncertainty clearly and avoid pretending incomplete work is complete.\n- Keep responses concise, useful, and grounded in observable evidence.\n"
            }
            Self::Identity => {
                "# IDENTITY\n\nUse this file to override Talos identity details for this workspace.\n\n## Identity\n- Add mission or role details only when they differ from the built-in Talos identity.\n"
            }
            Self::User => {
                "# USER\n\nUse this file for long-term user preferences.\n\n## Preferences\n- Add communication preferences here.\n"
            }
            Self::Heartbeat => {
                "# HEARTBEAT\n\nUse this file for long-running task update expectations.\n\n## Policy\n- Add cadence or escalation expectations for long tasks.\n"
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
            Self::Identity => prompt_config.bootstrap.include_identity,
            Self::User => prompt_config.bootstrap.include_user,
            Self::Heartbeat => prompt_config.bootstrap.include_heartbeat,
            Self::Memory => prompt_config.bootstrap.include_memory,
        };
        if !include_by_kind {
            return false;
        }
        match mode {
            PromptMode::Full => true,
            PromptMode::Minimal => {
                matches!(self, Self::Agents | Self::Soul | Self::User | Self::Memory)
            }
            PromptMode::None => false,
        }
    }

    pub(crate) fn should_auto_create(self) -> bool {
        matches!(self, Self::Soul | Self::User | Self::Memory)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBootstrapFile {
    pub kind: WorkspaceBootstrapFileKind,
    pub scope: BootstrapFileScope,
    pub path: PathBuf,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapFileScope {
    Workspace,
    ProjectRoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionKind {
    IdentityCore,
    RuntimeSessionContext,
    EvidenceToolingPolicy,
    ToolChoicePriority,
    RequestedToolPreferenceTemplate,
    RequestedToolPreferenceHint,
    FailureRecoveryPolicy,
    CompactToolRouting,
    ExpandedToolCatalog,
    TaskProtocolLibrary,
    CommunicationPolicy,
    PlanningInstruction,
    ProjectContext,
    RecentTaskContinuity,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CacheScope {
    Static,
    Workspace,
    // Default to the safest behavior for newly added sections until their reuse boundary is explicit.
    #[default]
    None,
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
    pub cache_scope: CacheScope,
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
    pub window_usage: PromptWindowUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_history_compaction: Option<SessionHistoryCompaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAssembly {
    pub static_prefix_sections: Vec<PromptSection>,
    pub dynamic_suffix_sections: Vec<PromptSection>,
    pub user_input: String,
    pub diagnostics: PromptDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityAnswerMode {
    CodeFirst,
    DocumentationFirst,
    DirectAnswer,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionContinuity {
    #[serde(default)]
    pub recent_verified_paths: Vec<String>,
    #[serde(default)]
    pub recent_successful_tools: Vec<String>,
    #[serde(default)]
    pub recent_failed_tools: Vec<String>,
    #[serde(default)]
    pub recent_unhelpful_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_answer_mode: Option<ContinuityAnswerMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PromptWarningState {
    #[serde(default)]
    pub seen_warning_keys: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PromptWindowUsage {
    pub window_resolved: bool,
    pub context_window: usize,
    pub reserved_output_tokens: usize,
    pub compaction_buffer_tokens: usize,
    pub effective_prompt_budget: usize,
    pub static_prefix_tokens: usize,
    pub dynamic_suffix_tokens: usize,
    pub total_tokens: usize,
    pub within_effective_budget: bool,
    pub cache_read_input_tokens: Option<u32>,
    pub cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHistoryCompaction {
    pub pre_compaction_total_tokens: usize,
    pub post_compaction_total_tokens: usize,
    pub recent_messages_before: usize,
    pub recent_messages_after: usize,
    pub summary_sections_after: usize,
    pub compaction_request_count: u32,
    pub compaction_input_tokens: u32,
    pub compaction_output_tokens: u32,
    pub compaction_total_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_cache_creation_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_cache_read_input_tokens: Option<u32>,
}

impl PromptWindowUsage {
    pub fn resolve(
        prompt_config: &AgentKernelPromptConfig,
        context_window: usize,
        model_max_output_tokens: usize,
        static_prefix_tokens: usize,
        dynamic_suffix_tokens: usize,
    ) -> Self {
        let reserved_output_tokens = resolve_reserved_output_tokens(
            &prompt_config.window.reserved_output_tokens,
            context_window,
            model_max_output_tokens,
        );
        let compaction_buffer_tokens = resolve_compaction_buffer_tokens(
            prompt_config.window.compaction_buffer_tokens,
            context_window,
        );
        let total_tokens = static_prefix_tokens + dynamic_suffix_tokens;
        let effective_prompt_budget = context_window
            .saturating_sub(reserved_output_tokens)
            .saturating_sub(compaction_buffer_tokens);
        let window_resolved = context_window > 0;

        Self {
            window_resolved,
            context_window,
            reserved_output_tokens,
            compaction_buffer_tokens,
            effective_prompt_budget,
            static_prefix_tokens,
            dynamic_suffix_tokens,
            total_tokens,
            within_effective_budget: !window_resolved || total_tokens <= effective_prompt_budget,
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
        }
    }
}

fn resolve_compaction_buffer_tokens(
    configured_buffer_tokens: usize,
    context_window: usize,
) -> usize {
    if context_window == 0 {
        return configured_buffer_tokens;
    }

    configured_buffer_tokens.min(context_window / 4)
}

fn resolve_reserved_output_tokens(
    policy: &ReservedOutputPolicy,
    context_window: usize,
    model_max_output_tokens: usize,
) -> usize {
    match policy {
        ReservedOutputPolicy::UseModelDefault => {
            if model_max_output_tokens > 0 {
                model_max_output_tokens
            } else {
                derive_reserved_output_tokens(context_window)
            }
        }
        ReservedOutputPolicy::Fixed { tokens } => *tokens,
        ReservedOutputPolicy::Ratio {
            numerator,
            denominator,
        } => {
            if *denominator == 0 {
                0
            } else {
                context_window.saturating_mul(*numerator as usize) / (*denominator as usize)
            }
        }
    }
}

fn derive_reserved_output_tokens(context_window: usize) -> usize {
    if context_window == 0 {
        return 0;
    }

    (context_window / 8).clamp(1_024, 8_192)
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
            session_recent_limit: 24,
            retrieved_memory_max_items: 4,
            retrieved_memory_max_chars: 2000,
            unified_recent_limit: 10,
        }
    }
}

/// Prompt build configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptBuildConfig {
    pub load_policy: PromptLoadPolicy,
    pub prompt: AgentKernelPromptConfig,
    pub memory: AgentKernelMemoryConfig,
}
