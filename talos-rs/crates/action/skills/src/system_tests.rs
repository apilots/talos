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
fn original_command_and_selection_resolve_the_protected_system_document() {
    let temp = tempfile::tempdir().unwrap();
    let roots = roots(&temp);
    for content in [
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
        let document = read_document(&roots, &selector).unwrap();
        assert_eq!(document.summary.scope, SkillScope::System);
        assert_eq!(selector.name.as_deref(), Some("loop"));
    }
}

#[test]
fn ordinary_prompts_discover_automation_in_the_existing_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let context = LocalSkillProvider::new(roots(&temp))
        .prompt_context(SkillPromptRequest {
            text: "用 loop 每 10 分钟检查 CI".into(),
            selected: Vec::new(),
            budget: SkillPromptBudget::default(),
        })
        .unwrap();
    assert!(context.available_summary.contains("goal [system]"));
    assert!(context.available_summary.contains("loop [system]"));
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
}
