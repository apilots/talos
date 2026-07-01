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
