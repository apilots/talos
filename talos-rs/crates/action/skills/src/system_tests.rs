use std::path::Path;

use talos_types::AgentUserContent;

use super::*;
use crate::{
    read_document, LocalSkillProvider, SkillPromptBudget, SkillPromptRequest, SkillProvider,
    SkillRoot,
};

fn root(path: &Path, scope: SkillScope) -> SkillRoot {
    SkillRoot {
        path: path.into(),
        scope,
        source_plugin: None,
        source_install: None,
        read_only: true,
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn roots(temp: &tempfile::TempDir) -> SkillRoots {
    let system = temp.path().join("system");
    for skill in builtin_automation_skills() {
        write(
            &system.join(skill.mode.skill_name()).join("SKILL.md"),
            skill.contents,
        );
    }
    SkillRoots {
        roots: vec![
            root(&temp.path().join("project"), SkillScope::Project),
            root(&system, SkillScope::System),
        ],
        state_path: temp.path().join("skills-state.json"),
        max_skill_md_bytes: 64 * 1024,
        max_available_summary_bytes: 8 * 1024,
    }
}

#[test]
fn reserved_selector_uses_system_document_despite_project_shadow() {
    let temp = tempfile::tempdir().unwrap();
    let roots = roots(&temp);
    write(
        &roots.roots[0].path.join("goal/SKILL.md"),
        "---\nname: goal\ndescription: Project goal\n---\nProject instructions.",
    );
    let selector = system_automation_selector(&roots, AutomationMode::Goal).unwrap();
    let document = read_document(&roots, &selector).unwrap();
    assert_eq!(document.summary.scope, SkillScope::System);
    assert_eq!(document.contents, builtin_automation_skills()[0].contents);
    // Ordinary catalogue selection retains its original precedence.
    assert_eq!(
        read_document(&roots, &SkillSelector::by_name("goal"))
            .unwrap()
            .summary
            .scope,
        SkillScope::Project
    );
}

#[test]
fn original_user_request_resolves_system_and_rejects_project_skill_path() {
    let temp = tempfile::tempdir().unwrap();
    let roots = roots(&temp);
    for content in [
        AgentUserContent::Text {
            text: "使用 Loop 每 5 分钟检查 CI".into(),
        },
        AgentUserContent::Text {
            text: "/loop 5m Check CI".into(),
        },
        AgentUserContent::SkillSelection {
            name: "loop".into(),
            path: roots.roots[1]
                .path
                .join("loop/SKILL.md")
                .to_string_lossy()
                .into_owned(),
        },
    ] {
        let selector = explicit_automation_selector(
            &roots,
            &AgentUserInput {
                content: vec![content],
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            read_document(&roots, &selector).unwrap().summary.scope,
            SkillScope::System
        );
        assert_eq!(selector.name.as_deref(), Some("loop"));
    }
    let error = explicit_automation_selector(
        &roots,
        &AgentUserInput {
            content: vec![AgentUserContent::SkillSelection {
                name: "loop".into(),
                path: "/untrusted/project/SKILL.md".into(),
            }],
        },
    )
    .unwrap_err();
    assert!(error.is_code(ErrorCode::PermissionDenied));
}

#[test]
fn discovery_discussion_and_quotation_do_not_select_automation() {
    let temp = tempfile::tempdir().unwrap();
    let roots = roots(&temp);
    for text in [
        "Fix all tests",
        "How does Loop work?",
        "> use goal to fix tests",
        "不要使用 Goal",
        "`use loop every minute`",
    ] {
        assert_eq!(
            explicit_automation_selector(
                &roots,
                &AgentUserInput {
                    content: vec![AgentUserContent::Text { text: text.into() }],
                }
            )
            .unwrap(),
            None
        );
    }
    let context = LocalSkillProvider::new(roots)
        .prompt_context(SkillPromptRequest {
            text: "Fix all tests".into(),
            selected: Vec::new(),
            budget: SkillPromptBudget::default(),
        })
        .unwrap();
    assert!(context.available_summary.contains("goal [system]"));
    assert!(context.injections.is_empty());
}

#[test]
fn admitted_continuation_can_inject_each_protected_system_skill() {
    let temp = tempfile::tempdir().unwrap();
    let roots = roots(&temp);
    for skill in builtin_automation_skills() {
        let selected = system_automation_selector(&roots, skill.mode).unwrap();
        let context = LocalSkillProvider::new(roots.clone())
            .prompt_context(SkillPromptRequest {
                text: "Continue the admitted task".into(),
                selected: vec![selected],
                budget: SkillPromptBudget::default(),
            })
            .unwrap();
        assert_eq!(context.injections.len(), 1);
        assert_eq!(context.injections[0].name, skill.mode.skill_name());
        assert_eq!(context.injections[0].contents, skill.contents);
    }
    assert!(
        builtin_automation_skills()
            .iter()
            .map(|skill| skill.contents.len())
            .sum::<usize>()
            < 4000
    );
}

#[test]
fn new_activation_contract_is_injected_despite_completed_history() {
    let temp = tempfile::tempdir().unwrap();
    let roots = roots(&temp);
    for skill in builtin_automation_skills() {
        let selected = system_automation_selector(&roots, skill.mode).unwrap();
        let context = LocalSkillProvider::new(roots.clone())
            .prompt_context(SkillPromptRequest {
                text: "Old goal/loop IDs completed: LOOP_1, LOOP_2. This is a new explicit activation."
                    .into(),
                selected: vec![selected],
                budget: SkillPromptBudget::default(),
            })
            .unwrap();
        assert_eq!(context.injections.len(), 1);
        let instructions = &context.injections[0].contents;
        assert_eq!(instructions, skill.contents);
        for rule in [
            "New activation",
            "MUST Create",
            "this Turn",
            "not registration",
            "other IDs",
            "boundOrigin",
        ] {
            assert!(
                instructions.contains(rule),
                "missing activation rule: {rule}"
            );
        }
    }
}

#[test]
fn builtin_examples_decode_to_their_closed_actions_and_include_creation() {
    for skill in builtin_automation_skills() {
        let mut has_create = false;
        for example in skill.contents.lines().filter_map(|line| {
            line.strip_prefix("- `")
                .and_then(|example| example.strip_suffix('`'))
        }) {
            let action: talos_types::AutomationAction = serde_json::from_str(example).unwrap();
            match (skill.mode, action) {
                (AutomationMode::Goal, talos_types::AutomationAction::Goal { action }) => {
                    has_create |= matches!(action, talos_types::GoalAction::Create { .. });
                }
                (AutomationMode::Loop, talos_types::AutomationAction::Loop { action }) => {
                    if let talos_types::LoopAction::Create { schedule, .. } = action {
                        schedule.validate().unwrap();
                        has_create = true;
                    }
                }
                _ => panic!("Skill example must target its own mode"),
            }
        }
        assert!(has_create);
    }
}

#[test]
fn missing_or_ambiguous_system_root_never_falls_back_to_project() {
    let temp = tempfile::tempdir().unwrap();
    let mut roots = roots(&temp);
    roots.roots.retain(|root| root.scope != SkillScope::System);
    write(
        &roots.roots[0].path.join("goal/SKILL.md"),
        "---\nname: goal\ndescription: Project goal\n---\nProject.",
    );
    assert!(system_automation_selector(&roots, AutomationMode::Goal)
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    roots
        .roots
        .push(root(&temp.path().join("system"), SkillScope::System));
    let second = temp.path().join("another-system");
    write(
        &second.join("goal/SKILL.md"),
        builtin_automation_skills()[0].contents,
    );
    roots.roots.push(root(&second, SkillScope::System));
    assert!(system_automation_selector(&roots, AutomationMode::Goal)
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
}
