use crate::{PromptSection, PromptSectionKind};

pub(crate) fn enrich_sections(sections: &mut [PromptSection]) {
    for section in sections {
        section.estimated_tokens = estimate_text_tokens(&section.content);
    }
}

pub(crate) fn truncate_sections_to_budget(
    sections: &mut Vec<PromptSection>,
    budget: usize,
    preserve_first: bool,
    dropped_sections: &mut Vec<PromptSectionKind>,
    truncated_sections: &mut Vec<PromptSectionKind>,
) {
    if sections.is_empty() {
        return;
    }

    while estimate_sections_tokens(sections) > budget {
        let removal_index = if preserve_first {
            (1..sections.len()).rev().find(|&index| !sections[index].required)
        } else {
            (0..sections.len()).find(|&index| !sections[index].required)
        };

        if let Some(index) = removal_index {
            let removed = sections.remove(index);
            dropped_sections.push(removed.kind);
            continue;
        }

        let truncation_index = if preserve_first {
            sections.len().saturating_sub(1)
        } else {
            0
        };
        let current = estimate_sections_tokens(sections);
        if current <= budget {
            break;
        }
        let overflow = current.saturating_sub(budget);
        let keep_tokens = sections[truncation_index]
            .estimated_tokens
            .saturating_sub(overflow)
            .max(1);
        if !truncated_sections.contains(&sections[truncation_index].kind) {
            truncated_sections.push(sections[truncation_index].kind);
        }
        sections[truncation_index].content =
            truncate_text_to_tokens(&sections[truncation_index].content, keep_tokens);
        sections[truncation_index].estimated_tokens =
            estimate_text_tokens(&sections[truncation_index].content);

        if estimate_sections_tokens(sections) >= current {
            break;
        }
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

pub(crate) fn truncate_text_to_tokens(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(4);
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    const SUFFIX: &str = "...[truncated]";
    if max_chars <= SUFFIX.len() {
        return SUFFIX.chars().take(max_chars).collect();
    }

    let keep_chars = max_chars - SUFFIX.len();
    let mut out = text.chars().take(keep_chars).collect::<String>();
    out.push_str(SUFFIX);
    out
}
