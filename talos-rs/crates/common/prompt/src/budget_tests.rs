use super::*;

fn section(kind: PromptSectionKind, content: &str) -> PromptSection {
    PromptSection {
        kind,
        payload: content.to_string(),
        role: None,
        estimated_tokens: estimate_text_tokens(content),
    }
}

#[test]
fn test_trim_dynamic_suffix_follows_spec_drop_order() {
    let uniform_content = "12345678";
    let mut sections = vec![
        section(PromptSectionKind::SessionSummary, uniform_content),
        section(
            PromptSectionKind::RequestedToolPreferenceHint,
            uniform_content,
        ),
        section(PromptSectionKind::RecentMessages, uniform_content),
        section(PromptSectionKind::ExpandedToolCatalog, uniform_content),
        section(PromptSectionKind::RecentTaskContinuity, uniform_content),
        section(PromptSectionKind::RecentToolOutputs, uniform_content),
    ];
    let section_cost = estimate_sections_tokens(&[section(
        PromptSectionKind::ExpandedToolCatalog,
        uniform_content,
    )]);
    let total_budget = estimate_sections_tokens(&sections) - (section_cost * 4);
    let mut dropped_sections = Vec::new();

    trim_dynamic_suffix_to_budget(&mut sections, total_budget, &mut dropped_sections);

    assert_eq!(
        dropped_sections,
        vec![
            PromptSectionKind::ExpandedToolCatalog,
            PromptSectionKind::RecentToolOutputs,
            PromptSectionKind::RecentMessages,
            PromptSectionKind::SessionSummary,
        ]
    );
    assert!(sections
        .iter()
        .all(|section| !dropped_sections.contains(&section.kind)));
}
