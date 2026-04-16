use crate::{PromptSection, PromptSectionKind};

const DYNAMIC_SUFFIX_DROP_ORDER: &[PromptSectionKind] = &[
    PromptSectionKind::ExpandedToolCatalog,
    PromptSectionKind::RetrievedMemories,
    PromptSectionKind::RecentMessages,
    PromptSectionKind::SessionSummary,
    PromptSectionKind::RequestedToolPreferenceHint,
    PromptSectionKind::RecentTaskContinuity,
    PromptSectionKind::ProjectContext,
];

pub(crate) fn enrich_sections(sections: &mut [PromptSection]) {
    for section in sections {
        section.estimated_tokens = estimate_text_tokens(&section.content);
    }
}

pub(crate) fn estimate_sections_tokens(sections: &[PromptSection]) -> usize {
    sections
        .iter()
        .map(|section| section.estimated_tokens + 4)
        .sum()
}

pub(crate) fn estimate_text_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    if chars == 0 {
        0
    } else {
        (chars + 3) / 4
    }
}

pub(crate) fn trim_dynamic_suffix_to_budget(
    sections: &mut Vec<PromptSection>,
    budget: usize,
    dropped_sections: &mut Vec<PromptSectionKind>,
) {
    if sections.is_empty() || estimate_sections_tokens(sections) <= budget {
        return;
    }

    for kind in DYNAMIC_SUFFIX_DROP_ORDER {
        while estimate_sections_tokens(sections) > budget {
            let Some(index) = sections
                .iter()
                .position(|section| section.kind == *kind && !section.required)
            else {
                break;
            };

            let removed = sections.remove(index);
            dropped_sections.push(removed.kind);
        }

        if estimate_sections_tokens(sections) <= budget {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BudgetTier, CacheScope, TruncationMode};

    fn section(kind: PromptSectionKind, content: &str) -> PromptSection {
        PromptSection {
            kind,
            title: None,
            content: content.to_string(),
            role: None,
            cache_scope: CacheScope::None,
            tier: BudgetTier::Supplemental,
            required: false,
            estimated_tokens: estimate_text_tokens(content),
            truncation_mode: TruncationMode::DropWhole,
        }
    }

    #[test]
    fn test_trim_dynamic_suffix_follows_spec_drop_order() {
        let uniform_content = "12345678";
        let mut sections = vec![
            section(PromptSectionKind::ProjectContext, uniform_content),
            section(PromptSectionKind::SessionSummary, uniform_content),
            section(
                PromptSectionKind::RequestedToolPreferenceHint,
                uniform_content,
            ),
            section(PromptSectionKind::RecentMessages, uniform_content),
            section(PromptSectionKind::ExpandedToolCatalog, uniform_content),
            section(PromptSectionKind::RecentTaskContinuity, uniform_content),
            section(PromptSectionKind::RetrievedMemories, uniform_content),
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
                PromptSectionKind::RetrievedMemories,
                PromptSectionKind::RecentMessages,
                PromptSectionKind::SessionSummary,
            ]
        );
        assert!(sections
            .iter()
            .all(|section| !dropped_sections.contains(&section.kind)));
    }
}
