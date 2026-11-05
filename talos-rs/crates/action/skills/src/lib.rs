pub mod catalog;
pub mod loader;
pub mod model;
pub mod prompt;
pub mod state;
mod system;

pub use catalog::{read_document, refresh, resolve, suggest};
pub use model::{
    SkillDocument, SkillLoadError, SkillLoadErrorKind, SkillMetadata, SkillRoot, SkillRoots,
    SkillScope, SkillSelector, SkillSnapshot, SkillSource, SkillSuggestion, SkillSummary,
    SkillWarning, SkillWarningCode,
};
pub use prompt::{
    LocalSkillProvider, SkillPromptBudget, SkillPromptContext, SkillPromptInjection,
    SkillPromptRequest, SkillPromptWarning, SkillProvider,
};
pub use state::{load_skill_state, PluginSkillStateKey, SkillState};
pub use system::{
    builtin_automation_skills, explicit_automation_selector, system_automation_selector,
    BuiltinAutomationSkill,
};

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
