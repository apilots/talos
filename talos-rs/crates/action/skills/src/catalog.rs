use crate::loader::{read_skill_contents, same_path};
use crate::model::{
    SkillDocument, SkillRoots, SkillSelector, SkillSnapshot, SkillSuggestion, SkillSummary,
    SkillWarning, SkillWarningCode,
};
use crate::state::load_skill_state;
use std::collections::HashSet;
use talos_error::{TalosError, TalosResult};

pub fn refresh(roots: &SkillRoots) -> TalosResult<SkillSnapshot> {
    let state = load_skill_state(&roots.state_path)?;
    Ok(crate::loader::load_skills_from_roots(
        &roots.roots,
        &state,
        roots.max_skill_md_bytes,
    ))
}

pub fn list(roots: &SkillRoots) -> TalosResult<Vec<SkillSummary>> {
    Ok(refresh(roots)?.skills)
}

pub fn suggest(
    roots: &SkillRoots,
    query: impl AsRef<str>,
    limit: usize,
) -> TalosResult<Vec<SkillSuggestion>> {
    let query = query.as_ref().trim().to_ascii_lowercase();
    let limit = limit.max(1);
    let mut suggestions = refresh(roots)?
        .skills
        .into_iter()
        .filter(|skill| skill.enabled)
        .filter_map(|skill| {
            skill_suggest_score(&skill, &query).map(|score| SkillSuggestCandidate { skill, score })
        })
        .collect::<Vec<_>>();
    if !query.is_empty() {
        suggestions.sort_by(|left, right| {
            left.score
                .cmp(&right.score)
                .then_with(|| left.skill.name.cmp(&right.skill.name))
                .then_with(|| left.skill.path.cmp(&right.skill.path))
        });
    }
    let suggestions = suggestions
        .into_iter()
        .take(limit)
        .map(|candidate| SkillSuggestion::from(&candidate.skill))
        .collect();
    Ok(suggestions)
}

struct SkillSuggestCandidate {
    skill: SkillSummary,
    score: usize,
}

fn skill_suggest_score(skill: &SkillSummary, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    [
        (0, skill.name.as_str()),
        (20, skill.short_description.as_deref().unwrap_or_default()),
        (40, skill.description.as_str()),
    ]
    .into_iter()
    .filter_map(|(base, value)| fuzzy_score(value, query).map(|score| base + score))
    .min()
}

fn fuzzy_score(value: &str, query: &str) -> Option<usize> {
    let value = value.to_ascii_lowercase();
    if value == query {
        return Some(0);
    }
    if value.starts_with(query) {
        return Some(5);
    }
    if let Some(index) = value.find(query) {
        return Some(15 + index);
    }
    subsequence_score(&value, query).map(|score| 50 + score)
}

fn subsequence_score(value: &str, query: &str) -> Option<usize> {
    let mut score = 0usize;
    let mut last_match: Option<usize> = None;
    let mut search_from = 0usize;
    for needle in query.chars() {
        let relative = value[search_from..].find(needle)?;
        let index = search_from + relative;
        score += relative;
        if let Some(last) = last_match {
            score += index.saturating_sub(last + 1);
        } else {
            score += index;
        }
        last_match = Some(index);
        search_from = index + needle.len_utf8();
    }
    Some(score)
}

pub fn read_document(roots: &SkillRoots, selector: &SkillSelector) -> TalosResult<SkillDocument> {
    let (summary, warnings) = select_skill(roots, selector, false)?;
    if !warnings.is_empty() {
        // Ambiguous name selectors are allowed for read after deterministic resolution.
    }
    let contents = read_skill_contents(&summary.path, roots.max_skill_md_bytes).map_err(|err| {
        TalosError::ResourceNotFound(format!(
            "failed to read skill `{}` at `{}`: {err}",
            summary.name,
            summary.path.display()
        ))
    })?;
    Ok(SkillDocument { summary, contents })
}

pub fn resolve(
    roots: &SkillRoots,
    text: impl AsRef<str>,
    selected: &[SkillSelector],
) -> TalosResult<(Vec<SkillSummary>, Vec<SkillWarning>)> {
    let mut resolved = Vec::new();
    let mut warnings = Vec::new();
    let mut blocked_plain_names = HashSet::new();
    for selector in selected {
        if let (Some(name), Some(_)) = (selector.name.as_ref(), selector.path.as_ref()) {
            let name = name.trim();
            if !name.is_empty() {
                blocked_plain_names.insert(name.to_string());
            }
        }
        match select_skill(roots, selector, true) {
            Ok((skill, mut selector_warnings)) => {
                push_unique(&mut resolved, skill);
                warnings.append(&mut selector_warnings);
            }
            Err(err) => warnings.push(SkillWarning {
                code: SkillWarningCode::SkillNotFound,
                message: err.to_string(),
            }),
        }
    }
    for name in extract_mentions(text.as_ref()) {
        if blocked_plain_names.contains(&name) {
            continue;
        }
        match select_skill(roots, &SkillSelector::by_name(name), true) {
            Ok((skill, mut selector_warnings)) => {
                push_unique(&mut resolved, skill);
                warnings.append(&mut selector_warnings);
            }
            Err(err) => warnings.push(SkillWarning {
                code: SkillWarningCode::SkillNotFound,
                message: err.to_string(),
            }),
        }
    }
    Ok((resolved, warnings))
}

pub(crate) fn select_skill(
    roots: &SkillRoots,
    selector: &SkillSelector,
    enabled_only: bool,
) -> TalosResult<(SkillSummary, Vec<SkillWarning>)> {
    let snapshot = refresh(roots)?;
    let mut matches = snapshot
        .skills
        .into_iter()
        .filter(|skill| !enabled_only || skill.enabled)
        .filter(|skill| match (&selector.name, &selector.path) {
            (_, Some(path)) => same_path(&skill.path, path),
            (Some(name), None) => skill.name == *name,
            (None, None) => false,
        })
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(TalosError::ResourceNotFound(format!(
            "skill selector `{selector:?}` did not match any enabled skill"
        )));
    }
    matches.sort_by(|left, right| {
        left.scope
            .priority()
            .cmp(&right.scope.priority())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
    });
    let selected = matches.remove(0);
    let warnings = if matches.is_empty() {
        Vec::new()
    } else {
        vec![SkillWarning {
            code: SkillWarningCode::AmbiguousSkillName,
            message: format!(
                "skill `{}` matched multiple sources; selected `{}`",
                selected.name,
                selected.path.display()
            ),
        }]
    };
    Ok((selected, warnings))
}

fn extract_mentions(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let (_, ch) = chars[index];
        if ch != '$' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < chars.len() {
            let (_, ch) = chars[end];
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                end += 1;
            } else {
                break;
            }
        }
        if end > start {
            let byte_start = chars[start].0;
            let byte_end = chars.get(end).map(|(pos, _)| *pos).unwrap_or(text.len());
            let name = text[byte_start..byte_end].trim_end_matches('.');
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
        index = end.max(index + 1);
    }
    names.sort();
    names.dedup();
    names
}

fn push_unique(skills: &mut Vec<SkillSummary>, skill: SkillSummary) {
    if !skills
        .iter()
        .any(|existing| same_path(&existing.path, &skill.path))
    {
        skills.push(skill);
    }
}
