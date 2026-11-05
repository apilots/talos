use super::*;

#[test]
fn original_leading_commands_and_typed_selections_enable_the_requested_mode() {
    for (text, expected) in [
        ("/goal repair tests\n/loop edit", AutomationMode::Goal),
        ("/loop 5m check CI", AutomationMode::Loop),
    ] {
        let input = AgentUserInput {
            content: vec![AgentUserContent::Text { text: text.into() }],
        };
        assert_eq!(explicit_automation_mode(&input).unwrap(), Some(expected));
    }
    let input = AgentUserInput {
        content: vec![
            AgentUserContent::Text {
                text: "Inspect the deployment".into(),
            },
            AgentUserContent::SkillSelection {
                name: "loop".into(),
                path: "/system/loop/SKILL.md".into(),
            },
        ],
    };
    assert_eq!(
        explicit_automation_mode(&input).unwrap(),
        Some(AutomationMode::Loop)
    );
}

#[test]
fn ordinary_user_text_is_available_to_model_intent_without_command_parsing() {
    let input = AgentUserInput {
        content: vec![AgentUserContent::Text {
            text: "Use the loop index to fix the off-by-one\n用goal描述函数目标\n/loop edit".into(),
        }],
    };
    assert_eq!(explicit_automation_mode(&input).unwrap(), None);
}

#[test]
fn shared_commands_preserve_the_complete_task_prompt() {
    for prompt in [
        "5m check CI and report changes",
        "每两小时检查一次",
        "wait until deployment changes",
    ] {
        assert_eq!(
            resolve_automation_command(&format!("/loop {prompt}")).unwrap(),
            Some(AutomationCommand::Activate {
                mode: AutomationMode::Loop,
                prompt: prompt.into()
            }),
        );
    }
}
