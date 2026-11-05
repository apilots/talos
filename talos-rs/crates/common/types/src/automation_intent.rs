//! Conservative activation rules for original, User-authored input only.
//!
//! This is a business command recognizer, not general language understanding.
//! Ambiguous discussion stays ordinary input. Workers resolve the returned mode
//! to the protected System Skill; a matching name is not a trusted filesystem path.

use talos_error::{ErrorCode, TalosError, TalosResult};

use crate::{
    resolve_automation_command, AgentUserContent, AgentUserInput, AutomationCommand, AutomationMode,
};

pub fn explicit_automation_mode(input: &AgentUserInput) -> TalosResult<Option<AutomationMode>> {
    let mut selected = None;
    for content in &input.content {
        match content {
            AgentUserContent::SkillSelection { name, .. } => {
                let mode = match name.as_str() {
                    "goal" => Some(AutomationMode::Goal),
                    "loop" => Some(AutomationMode::Loop),
                    _ => None,
                };
                merge_mode(&mut selected, mode)?;
            }
            AgentUserContent::Text { text } => {
                let mut fenced = false;
                for line in text.lines().map(str::trim) {
                    if line.starts_with("```") || line.starts_with("~~~") {
                        fenced = !fenced;
                        continue;
                    }
                    if fenced || line.starts_with(['>', '"', '\'', '“', '「', '`']) {
                        continue;
                    }
                    let mode = match resolve_automation_command(line)? {
                        Some(AutomationCommand::Activate { mode, .. }) => Some(mode),
                        Some(_) => None,
                        None => requested_mode(line),
                    };
                    merge_mode(&mut selected, mode)?;
                }
            }
            _ => {}
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

fn requested_mode(line: &str) -> Option<AutomationMode> {
    let text = line.to_ascii_lowercase();
    let mut text = text.trim();
    // A polite request may contain several prefixes or end in a question mark.
    // The capability still has to follow an explicit invocation verb.
    while let Some(rest) = [
        "please ",
        "could you ",
        "can you ",
        "would you ",
        "help me ",
        "请",
        "帮我",
        "帮忙",
        "麻烦",
        "我想",
        "我希望",
        "希望",
        "你能",
        "能否",
    ]
    .iter()
    .find_map(|prefix| text.strip_prefix(prefix))
    {
        text = rest.trim_start();
    }
    let text = ["使用", "用", "启用", "开启", "use ", "start ", "enable "]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))?
        .trim_start();
    // This only identifies the requested capability, never parses its cadence.
    let text = [
        "模型决定间隔的",
        "模型决定的",
        "固定间隔的",
        "model-decided ",
        "the ",
    ]
    .iter()
    .find_map(|prefix| text.strip_prefix(prefix))
    .unwrap_or(text)
    .trim_start();
    for (name, mode) in [
        ("goal", AutomationMode::Goal),
        ("loop", AutomationMode::Loop),
    ] {
        if let Some(rest) = text.strip_prefix(name) {
            if !rest.is_empty()
                && !rest.starts_with(|character: char| {
                    character.is_ascii_alphanumeric() || character == '_'
                })
                && !rest.trim_start().starts_with(['"', '”'])
            {
                let task = rest.trim_start_matches(|character: char| {
                    character.is_whitespace() || matches!(character, ':' | '：' | ',' | '，')
                });
                let task = task.trim_end_matches(['?', '？', '.', '。', '!', '！']);
                let capability_inquiry = ["支持", "会", "能", "有", "support "]
                    .iter()
                    .any(|prefix| task.starts_with(prefix))
                    && [
                        "什么", "哪些", "多少", "怎么", "如何", "啥", "what ", "which ", "how ",
                    ]
                    .iter()
                    .any(|question| task.contains(question));
                // Asking what a capability is, or how to configure it, is not
                // a task invocation. Keep bare permission questions ordinary.
                if task.is_empty()
                    || capability_inquiry
                    || matches!(task, "吗" | "可以吗" | "好吗" | "行吗")
                    || [
                        "是什么",
                        "怎么",
                        "如何",
                        "是否",
                        "会不会",
                        "有什么",
                        "和 loop",
                        "和loop",
                        "与 loop",
                        "与loop",
                        "和 goal",
                        "和goal",
                        "与 goal",
                        "与goal",
                        "what ",
                        "how ",
                        "why ",
                        "does ",
                        "can ",
                        "will ",
                        "vs ",
                    ]
                    .iter()
                    .any(|prefix| task.starts_with(prefix))
                {
                    return None;
                }
                return Some(mode);
            }
        }
    }
    None
}
