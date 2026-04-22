//! Talos Prompt - Prompt Template Engine
//!
//! This crate exposes a thin facade over the prompt assembly modules.

mod assembler;
mod bootstrap;
mod budget;
mod builder;
mod context;
mod diagnostics;
mod manager;
pub mod presets;
mod regions;
mod renderer;
mod template;
mod types;

#[cfg(test)]
mod tests;

pub use builder::{render_prompt_assembly, render_prompt_blocks, PromptBuilder};
pub use context::PromptContext;
pub use manager::PromptManager;
pub use regions::{
    bootstrap_section_kind, region_template, PromptSectionTemplate, PROMPT_ARCHITECTURE_VERSION,
};
pub use template::PromptTemplate;
pub use types::{
    compute_prompt_fingerprint, determine_prompt_cache_break_reason, BootstrapFileDiagnostic,
    BootstrapFileScope, BudgetTier, ContinuityAnswerMode, PlannerMode, PromptAssembly, PromptBlock,
    PromptBlockCacheScope, PromptBlockKind, PromptBuildConfig, PromptCacheBreakReason,
    PromptCacheState, PromptDiagnostics, PromptLoadPolicy, PromptMode, PromptPhase, PromptSection,
    PromptSectionKind, PromptWarningState, PromptWindowUsage, RegionEnvelopePolicy,
    RegionPlacement, RegionPriority, RequestContextInjection, SectionCacheScope, SessionContinuity,
    SessionHistoryCompaction, TruncationMode, WorkspaceBootstrapFile, WorkspaceBootstrapFileKind,
};
