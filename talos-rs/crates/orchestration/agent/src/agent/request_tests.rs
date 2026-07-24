use super::*;
use std::path::PathBuf;

#[test]
fn extracts_structured_skill_selections_with_path() {
    let selections = AgentKernelParams::extract_skill_selections(&[
        TalosInputItem::Text {
            text: "Use $review".to_string(),
        },
        TalosInputItem::SkillSelection {
            name: " review ".to_string(),
            path: " /tmp/review/SKILL.md ".to_string(),
        },
        TalosInputItem::SkillSelection {
            name: "ignored".to_string(),
            path: " ".to_string(),
        },
    ]);

    assert_eq!(
        selections,
        vec![SkillSelector::selection(
            "review",
            PathBuf::from("/tmp/review/SKILL.md")
        )]
    );
}

#[test]
fn extracts_one_normalized_user_input_without_losing_file_order() {
    let input = AgentKernelParams::extract_user_input(&[
        TalosInputItem::Text {
            text: " first ".to_string(),
        },
        TalosInputItem::Files {
            file_ids: vec!["file-2".to_string(), "file-1".to_string()],
        },
        TalosInputItem::Text {
            text: "second".to_string(),
        },
    ])
    .expect("user input should normalize");

    assert_eq!(
        input,
        talos_agent_kernel::UserInput {
            text: "first\n\nsecond".to_string(),
            file_ids: vec!["file-2".to_string(), "file-1".to_string()],
        }
    );
}

#[test]
fn rejects_multiple_files_items_at_the_agent_boundary() {
    assert!(AgentKernelParams::extract_user_input(&[
        TalosInputItem::Files {
            file_ids: vec!["file-1".to_string()],
        },
        TalosInputItem::Files {
            file_ids: vec!["file-2".to_string()],
        },
    ])
    .is_err());
}
