use super::{validate_entry, KnowledgeDocument};
use crate::{
    KnowledgeDocumentProfile, KnowledgeEntry, KnowledgeFormatError, KnowledgeFormatErrorKind,
    KnowledgeFormatResult, OkfDocument, SectionKey,
};

pub(super) fn parse_document(
    profile: &KnowledgeDocumentProfile,
    content: &str,
) -> KnowledgeFormatResult<KnowledgeDocument> {
    if content.chars().count() > profile.limits.max_chars {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::LimitExceeded,
            "managed document source exceeds the profile limit",
        ));
    }
    let okf = OkfDocument::parse(content)?;
    if !okf.validate().is_conformant() || okf.frontmatter() != profile.frontmatter {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidDocument,
            "managed document frontmatter does not match its profile",
        ));
    }
    let body = okf.body();
    let start = profile.start_marker();
    let end = profile.end_marker();
    if body.matches(&start).count() != 1 || body.matches(&end).count() != 1 {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidDocument,
            "managed document must contain exactly one marker pair",
        ));
    }
    let start_at = body.find(&start).expect("validated start marker");
    let after_start = start_at + start.len();
    let relative_end = body[after_start..].find(&end).ok_or_else(|| {
        KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidDocument,
            "managed document markers are out of order",
        )
    })?;
    let end_at = after_start + relative_end;
    let prefix = body[..start_at].trim_end().to_string();
    let suffix = body[end_at + end.len()..].trim().to_string();
    let entries = parse_entries(profile, &body[after_start..end_at])?;
    Ok(KnowledgeDocument {
        profile: profile.clone(),
        okf,
        prefix,
        suffix,
        entries,
    })
}

fn parse_entries(
    profile: &KnowledgeDocumentProfile,
    region: &str,
) -> KnowledgeFormatResult<Vec<KnowledgeEntry>> {
    let lines: Vec<&str> = region.lines().collect();
    let marker_prefix = profile.entry_marker_prefix();
    let mut current_section: Option<SectionKey> = None;
    let mut next_section_index = 0;
    let mut entries = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if let Some(title) = line.strip_prefix("## ") {
            let Some(expected) = profile.sections.get(next_section_index) else {
                return Err(KnowledgeFormatError::new(
                    KnowledgeFormatErrorKind::InvalidDocument,
                    format!("unexpected managed section: {title}"),
                ));
            };
            if title.trim() != expected.title {
                return Err(KnowledgeFormatError::new(
                    KnowledgeFormatErrorKind::InvalidDocument,
                    format!(
                        "managed section `{}` must appear before `{}`",
                        expected.title,
                        title.trim()
                    ),
                ));
            }
            current_section = Some(expected.key.clone());
            next_section_index += 1;
            index += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix(&marker_prefix) {
            let key = rest.strip_suffix("\" -->").ok_or_else(|| {
                KnowledgeFormatError::new(
                    KnowledgeFormatErrorKind::InvalidDocument,
                    "malformed knowledge entry marker",
                )
            })?;
            let section = current_section.clone().ok_or_else(|| {
                KnowledgeFormatError::new(
                    KnowledgeFormatErrorKind::InvalidDocument,
                    "knowledge entry appears before a section",
                )
            })?;
            index += 1;
            while index < lines.len() && lines[index].trim().is_empty() {
                index += 1;
            }
            let title = lines
                .get(index)
                .and_then(|line| line.trim().strip_prefix("### "))
                .ok_or_else(|| {
                    KnowledgeFormatError::new(
                        KnowledgeFormatErrorKind::InvalidDocument,
                        "knowledge entry title is missing",
                    )
                })?
                .trim()
                .to_string();
            index += 1;
            let body_start = index;
            while index < lines.len()
                && !lines[index].trim().starts_with(&marker_prefix)
                && !lines[index].trim().starts_with("## ")
            {
                index += 1;
            }
            let body = lines[body_start..index].join("\n").trim().to_string();
            let entry = KnowledgeEntry::builder()
                .key(key)
                .section(section.as_str())
                .title(title)
                .body(body)
                .build()?;
            validate_entry(profile, &entry)?;
            entries.push(entry);
            continue;
        }
        if line.is_empty() {
            index += 1;
            continue;
        }
        if line.starts_with("<!--") {
            index = skip_html_comment(&lines, index)?;
            continue;
        }
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidDocument,
            format!("unexpected content in managed region: {line}"),
        ));
    }
    if next_section_index != profile.sections.len() {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidDocument,
            "managed document is missing one or more required sections",
        ));
    }
    Ok(entries)
}

fn skip_html_comment(lines: &[&str], start: usize) -> KnowledgeFormatResult<usize> {
    let mut index = start;
    while index < lines.len() {
        if lines[index].contains("-->") {
            return Ok(index + 1);
        }
        index += 1;
    }
    Err(KnowledgeFormatError::new(
        KnowledgeFormatErrorKind::InvalidDocument,
        "unterminated comment in managed region",
    ))
}
