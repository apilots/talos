//! The two shipped automation documents and their protected System selectors.
//! Filesystem provisioning belongs to the existing Plugin installation owner.

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{explicit_automation_mode, AgentUserContent, AgentUserInput, AutomationMode};

use crate::{refresh, SkillRoots, SkillScope, SkillSelector};

pub struct BuiltinAutomationSkill {
    pub mode: AutomationMode,
    pub contents: &'static str,
}

const AUTOMATION_SKILLS: &[BuiltinAutomationSkill] = &[
    BuiltinAutomationSkill {
        mode: AutomationMode::Goal,
        contents: include_str!("../builtins/goal/SKILL.md"),
    },
    BuiltinAutomationSkill {
        mode: AutomationMode::Loop,
        contents: include_str!("../builtins/loop/SKILL.md"),
    },
];

/// Shipped content used by PluginManager under its original state lock.
pub fn builtin_automation_skills() -> &'static [BuiltinAutomationSkill] {
    AUTOMATION_SKILLS
}

/// Resolve a reserved entry or admitted automatic origin to its System document.
/// Project, Plugin and External names/paths cannot shadow this selection.
/// This is prompt selection only; the Service still authorizes each operation.
pub fn system_automation_selector(
    roots: &SkillRoots,
    mode: AutomationMode,
) -> TalosResult<SkillSelector> {
    let mut matches = refresh(roots)?.skills.into_iter().filter(|skill| {
        skill.scope == SkillScope::System && skill.read_only && skill.name == mode.skill_name()
    });
    let skill = matches.next().ok_or_else(|| {
        TalosError::new(
            ErrorCode::ResourceNotFound,
            "Built-in automation Skill is unavailable",
        )
    })?;
    if matches.next().is_some() {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "Built-in automation Skill has multiple System definitions",
        ));
    }
    Ok(SkillSelector::selection(skill.name, skill.path))
}

/// Resolve only explicit original User intent, not model prose or discovery.
/// The caller must supply original User input, never generated automatic input.
pub fn explicit_automation_selector(
    roots: &SkillRoots,
    input: &AgentUserInput,
) -> TalosResult<Option<SkillSelector>> {
    let Some(mode) = explicit_automation_mode(input)? else {
        return Ok(None);
    };
    let selected = system_automation_selector(roots, mode)?;
    for content in &input.content {
        if let AgentUserContent::SkillSelection { name, path } = content {
            if name == mode.skill_name()
                && !selected.path.as_ref().is_some_and(|expected| {
                    crate::loader::same_path(expected, std::path::Path::new(path))
                })
            {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Automation selection must identify the built-in System Skill",
                ));
            }
        }
    }
    Ok(Some(selected))
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
