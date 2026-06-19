use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use talos_config::{
    default_timezone, AgentKernelMemoryConfig, AgentKernelPromptConfig, ReservedOutputPolicy,
    ToolsRuntimeConfig,
};
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
pub enum DynamicTrimPolicy {
    #[default]
    TrimToBudget,
    PreserveAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannerMode {
    None,
    #[default]
    BuiltIn,
}

impl PlannerMode {
    pub(crate) fn parse(value: &str) -> TalosResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "built_in" => Ok(Self::BuiltIn),
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
    EvidenceToolingPolicy,
    ToolChoicePriority,
    RequestedToolPreferenceTemplate,
    RequestedToolPreferenceHint,
    FailureRecoveryPolicy,
    ExpandedToolCatalog,
    SkillCatalog,
    ActiveSkillInstructions,
    TaskProtocolLibrary,
    CommunicationPolicy,
    PlanningInstruction,
    BootstrapSoul,
    BootstrapIdentity,
    BootstrapUser,
    BootstrapHeartbeat,
    BootstrapMemory,
    BootstrapAgents,
    RecentTaskContinuity,
    SessionSummary,
    RecentMessages,
    RecentToolHistory,
    RecentToolOutputs,
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
pub enum SectionCacheScope {
    StaticPrefixEligible,
    WorkspaceStable,
    #[default]
    DynamicOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionPlacement {
    StaticPrefixRequired,
    DynamicSuffixRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionEnvelopePolicy {
    PlainTextRequired,
    XmlEnvelopeRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionPriority {
    Highest,
    High,
    MediumHigh,
    Medium,
    MediumLow,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSection {
    pub kind: PromptSectionKind,
    pub payload: String,
    pub role: Option<TalosMessageRole>,
    pub estimated_tokens: usize,
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
    pub request_context_injection: RequestContextInjection,
    pub user_input: String,
    pub diagnostics: PromptDiagnostics,
    pub cache_state: PromptCacheState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestContextInjection {
    pub appended_system_context: Option<String>,
    pub prepended_user_context_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptBlockKind {
    Attribution,
    StaticPrefix,
    DynamicSuffix,
    AppendedSystemContext,
    PrependedUserContextMessage,
    UserInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptBlockCacheScope {
    StablePrefix,
    ScopedReusable,
    NoCache,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptBlock {
    pub kind: PromptBlockKind,
    pub text: String,
    pub cache_scope: PromptBlockCacheScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheBreakReason {
    PromptVersionChanged,
    ToolCatalogChanged,
    BootstrapChanged,
    ModelChanged,
    ManualClear,
    ManualCompact,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptCacheState {
    pub prompt_version: String,
    pub static_prefix_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_catalog_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_break_reason: Option<PromptCacheBreakReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityAnswerMode {
    CodeFirst,
    DocumentationFirst,
    DirectAnswer,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityToolOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuityToolSummary {
    pub tool_name: String,
    pub outcome: ContinuityToolOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedPathEvidence {
    pub path: String,
    pub source_tool: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionContinuity {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recent_goal: String,
    #[serde(default)]
    pub verified_paths: Vec<VerifiedPathEvidence>,
    #[serde(default)]
    pub tool_summaries: Vec<ContinuityToolSummary>,
    #[serde(default)]
    pub pending_tasks: Vec<String>,
    #[serde(default)]
    pub recent_unhelpful_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_answer_mode: Option<ContinuityAnswerMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_next_step: Option<String>,
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
#[serde(rename_all = "snake_case")]
pub enum CompactionTrimUnitKind {
    OldestHistoryItem,
    OldToolOutput,
    OldAssistantToolExchange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionHistoryCompactionTrigger {
    ThresholdExceeded,
    HardThresholdExceeded,
    BudgetExceeded,
    HistorySectionsDropped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionHistoryCompactionOutcome {
    Applied,
    RejectedMissingSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHistoryCompaction {
    pub trigger: SessionHistoryCompactionTrigger,
    pub outcome: SessionHistoryCompactionOutcome,
    pub pre_compaction_total_tokens: usize,
    pub post_compaction_total_tokens: usize,
    pub recent_messages_before: usize,
    pub recent_messages_after: usize,
    pub summary_sections_after: usize,
    pub trimmed_context_unit_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trimmed_context_unit_kind: Option<CompactionTrimUnitKind>,
    pub fallback_section_drop_applied: bool,
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
        static_prefix_tokens: usize,
        dynamic_suffix_tokens: usize,
    ) -> Self {
        let reserved_output_tokens = resolve_reserved_output_tokens(
            &prompt_config.window.reserved_output_tokens,
            context_window,
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

fn resolve_reserved_output_tokens(policy: &ReservedOutputPolicy, context_window: usize) -> usize {
    match policy {
        ReservedOutputPolicy::UseModelDefault => derive_reserved_output_tokens(context_window),
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

    (context_window / 8).clamp(1_024, 8_192).min(context_window)
}

/// Prompt context loading policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptLoadPolicy {
    pub include_session_summary: bool,
    pub include_recent_messages: bool,
    pub include_recent_tool_history: bool,
    pub include_recent_tool_outputs: bool,
    pub include_retrieved_memories: bool,
    pub session_recent_limit: usize,
    pub session_recent_max_chars: usize,
    pub recent_tool_history_limit: usize,
    pub recent_tool_history_max_chars: usize,
    pub recent_tool_outputs_limit: usize,
    pub recent_tool_outputs_max_chars: usize,
    pub session_summary_recent_count: usize,
    pub session_summary_max_chars: usize,
    pub retrieved_memory_max_items: usize,
    pub retrieved_memory_max_chars: usize,
    pub unified_recent_limit: usize,
}

impl Default for PromptLoadPolicy {
    fn default() -> Self {
        Self {
            include_session_summary: true,
            include_recent_messages: true,
            include_recent_tool_history: true,
            include_recent_tool_outputs: true,
            include_retrieved_memories: true,
            session_recent_limit: 24,
            session_recent_max_chars: 4_000,
            recent_tool_history_limit: 8,
            recent_tool_history_max_chars: 600,
            recent_tool_outputs_limit: 8,
            recent_tool_outputs_max_chars: 2_000,
            session_summary_recent_count: 2,
            session_summary_max_chars: 1_200,
            retrieved_memory_max_items: 4,
            retrieved_memory_max_chars: 2000,
            unified_recent_limit: 10,
        }
    }
}

/// Prompt build configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptBuildConfig {
    pub load_policy: PromptLoadPolicy,
    #[serde(default)]
    pub workspace_dir: String,
    pub prompt: AgentKernelPromptConfig,
    pub memory: AgentKernelMemoryConfig,
    pub tools: ToolsRuntimeConfig,
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

impl Default for PromptBuildConfig {
    fn default() -> Self {
        Self {
            load_policy: PromptLoadPolicy::default(),
            workspace_dir: String::new(),
            prompt: AgentKernelPromptConfig::default(),
            memory: AgentKernelMemoryConfig::default(),
            tools: ToolsRuntimeConfig::default(),
            timezone: default_timezone(),
        }
    }
}

impl PromptSection {
    pub fn is_required(&self) -> bool {
        crate::regions::region_template(self.kind).required
    }

    pub fn cache_scope(&self) -> SectionCacheScope {
        crate::regions::region_template(self.kind).cache_scope
    }

    pub fn tier(&self) -> BudgetTier {
        crate::regions::region_template(self.kind).tier
    }

    pub fn placement(&self) -> RegionPlacement {
        crate::regions::region_template(self.kind).placement
    }

    pub fn envelope_policy(&self) -> RegionEnvelopePolicy {
        crate::regions::region_template(self.kind).envelope_policy
    }

    pub fn title(&self) -> &'static str {
        crate::regions::region_template(self.kind).title
    }

    pub fn framing_prompt(&self) -> &'static str {
        crate::regions::region_template(self.kind).framing_prompt
    }

    pub fn stable_name(&self) -> &'static str {
        crate::regions::region_template(self.kind).stable_name
    }

    pub fn region_role(&self) -> &'static str {
        crate::regions::region_template(self.kind).role
    }

    pub fn priority(&self) -> RegionPriority {
        crate::regions::region_template(self.kind).priority
    }
}

impl PromptBlockCacheScope {
    pub fn is_cacheable(self) -> bool {
        !matches!(self, Self::NoCache)
    }
}

impl RegionPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Highest => "highest",
            Self::High => "high",
            Self::MediumHigh => "medium_high",
            Self::Medium => "medium",
            Self::MediumLow => "medium_low",
            Self::Low => "low",
        }
    }
}

pub fn compute_prompt_fingerprint(parts: &[&str]) -> String {
    let mut hasher = DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

pub fn determine_prompt_cache_break_reason(
    previous: &PromptCacheState,
    current: &PromptCacheState,
) -> Option<PromptCacheBreakReason> {
    if previous.prompt_version != current.prompt_version {
        return Some(PromptCacheBreakReason::PromptVersionChanged);
    }
    if previous.model_fingerprint != current.model_fingerprint {
        return Some(PromptCacheBreakReason::ModelChanged);
    }
    if previous.bootstrap_fingerprint != current.bootstrap_fingerprint {
        return Some(PromptCacheBreakReason::BootstrapChanged);
    }
    if previous.tool_catalog_fingerprint != current.tool_catalog_fingerprint {
        return Some(PromptCacheBreakReason::ToolCatalogChanged);
    }
    if previous.static_prefix_fingerprint != current.static_prefix_fingerprint {
        return Some(PromptCacheBreakReason::PromptVersionChanged);
    }
    None
}
