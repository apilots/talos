//! Deterministic activation from an original leading command or typed selection.
//! Plain prompt intent belongs to the model's built-in Skill instructions.

use talos_error::{ErrorCode, TalosError, TalosResult};

use crate::{
    resolve_automation_command, AgentUserContent, AgentUserInput, AutomationCommand, AutomationMode,
};

pub fn explicit_automation_mode(input: &AgentUserInput) -> TalosResult<Option<AutomationMode>> {
    let mut selected = None;
    for content in &input.content {
        if let AgentUserContent::SkillSelection { name, .. } = content {
            let mode = match name.as_str() {
                "goal" => Some(AutomationMode::Goal),
                "loop" => Some(AutomationMode::Loop),
                _ => None,
            };
            merge_mode(&mut selected, mode)?;
        }
    }
    if let Some(text) = input.content.iter().find_map(|content| match content {
        AgentUserContent::Text { text } => Some(text),
        _ => None,
    }) {
        if let Some(AutomationCommand::Activate { mode, .. }) =
            resolve_automation_command(text.trim_start())?
        {
            merge_mode(&mut selected, Some(mode))?;
        }
    }
    Ok(selected)
}

fn merge_mode(
    selected: &mut Option<AutomationMode>,
    mode: Option<AutomationMode>,
) -> TalosResult<()> {
    if let Some(mode) = mode {
        if selected.is_some_and(|current| current != mode) {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Goal and Loop cannot be enabled together",
            ));
        }
        *selected = Some(mode);
    }
    Ok(())
}
