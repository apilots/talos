use crate::{regions, PromptSection, PromptSectionKind};

pub(crate) fn enrich_sections(sections: &mut [PromptSection]) {
    for section in sections {
        section.estimated_tokens =
            estimate_text_tokens(&crate::renderer::render_section_text(section));
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
        chars.div_ceil(4)
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

    for kind in regions::DYNAMIC_SUFFIX_DROP_ORDER {
        while estimate_sections_tokens(sections) > budget {
            let Some(index) = sections
                .iter()
                .position(|section| section.kind == *kind && !section.is_required())
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
#[path = "budget_tests.rs"]
mod tests;
