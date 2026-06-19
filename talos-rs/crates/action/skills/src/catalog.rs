use crate::loader::{read_skill_contents, same_path};
use crate::model::{
    SkillDocument, SkillRoots, SkillSelector, SkillSnapshot, SkillSuggestion, SkillSummary,
    SkillWarning,
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
    let suggestions = refresh(roots)?
        .skills
        .into_iter()
        .filter(|skill| skill.enabled)
        .filter(|skill| {
            query.is_empty()
                || skill.name.to_ascii_lowercase().contains(&query)
                || skill.description.to_ascii_lowercase().contains(&query)
                || skill
                    .short_description
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .take(limit)
        .map(|skill| SkillSuggestion::from(&skill))
        .collect();
    Ok(suggestions)
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
        if let SkillSelector::Selection { name, .. } = selector {
            if !name.trim().is_empty() {
                blocked_plain_names.insert(name.clone());
            }
        }
        match select_skill(roots, selector, true) {
            Ok((skill, mut selector_warnings)) => {
                push_unique(&mut resolved, skill);
                warnings.append(&mut selector_warnings);
            }
            Err(err) => warnings.push(SkillWarning {
                code: "skill_not_found".to_string(),
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
                code: "skill_not_found".to_string(),
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
        .filter(|skill| match selector {
            SkillSelector::Selection { path, .. } => same_path(&skill.path, path),
            SkillSelector::ByPath { path } => same_path(&skill.path, path),
            SkillSelector::ByName { name } => skill.name == *name,
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
            code: "ambiguous_skill_name".to_string(),
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
