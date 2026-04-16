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
mod renderer;
mod template;
mod types;

#[cfg(test)]
mod tests;

pub use builder::{render_prompt_assembly, PromptBuilder};
pub use context::PromptContext;
pub use manager::PromptManager;
pub use template::PromptTemplate;
pub use types::{
    BootstrapFileDiagnostic, BootstrapFileScope, BudgetTier, CacheScope, ContinuityAnswerMode,
    PlannerMode, PromptAssembly, PromptBuildConfig, PromptDiagnostics, PromptLoadPolicy,
    PromptMode, PromptPhase, PromptSection, PromptSectionKind, PromptWarningState,
    PromptWindowUsage, SessionContinuity, SessionHistoryCompaction, TruncationMode,
    WorkspaceBootstrapFile, WorkspaceBootstrapFileKind,
};
