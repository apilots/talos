use std::collections::BTreeSet;

use super::parser::parse_document;
use super::renderer::{empty_template, remove_html_comments, render_body};
use crate::managed_document::profile::{validate_key, validate_single_line};
use crate::{
    KnowledgeDocumentProfile, KnowledgeEntry, KnowledgeEntryQuery, KnowledgeEntryUpdate,
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult, OkfDocument,
};

/// Immutable parsed managed document.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeDocument {
    pub(super) profile: KnowledgeDocumentProfile,
    pub(super) okf: OkfDocument,
    pub(super) prefix: String,
    pub(super) suffix: String,
    pub(super) entries: Vec<KnowledgeEntry>,
}

impl KnowledgeDocument {
    /// Starts the unified builder facade.
    #[must_use]
    pub fn builder(profile: &KnowledgeDocumentProfile) -> KnowledgeDocumentBuilder {
        KnowledgeDocumentBuilder {
            profile: profile.clone(),
            source: None,
            commands: Vec::new(),
        }
    }

    /// Returns one entry by stable key.
    #[must_use]
    pub fn get_entry(&self, key: &str) -> Option<&KnowledgeEntry> {
        self.entries.iter().find(|entry| entry.key.as_str() == key)
    }

    /// Lists entries in profile section order and file order.
    #[must_use]
    pub fn list_entries(&self, query: KnowledgeEntryQuery) -> Vec<&KnowledgeEntry> {
        let mut entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| {
                query
                    .section
                    .as_ref()
                    .is_none_or(|section| &entry.section == section)
                    && query
                        .key_prefix
                        .as_ref()
                        .is_none_or(|prefix| entry.key.as_str().starts_with(prefix))
            })
            .collect();
        if let Some(limit) = query.limit {
            entries.truncate(limit);
        }
        entries
    }

    /// Renders canonical OKF Markdown and enforces the profile cap.
    pub fn render(&self) -> KnowledgeFormatResult<String> {
        let body = render_body(self);
        let rendered = OkfDocument::builder()
            .frontmatter(self.okf.frontmatter())
            .body(body)
            .build()?
            .render();
        if rendered.chars().count() > self.profile.limits.max_chars {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::LimitExceeded,
                format!(
                    "managed document exceeds {} characters",
                    self.profile.limits.max_chars
                ),
            ));
        }
        Ok(rendered)
    }

    /// Projects user-visible content without frontmatter or control comments.
    pub fn prompt_view(&self) -> KnowledgeFormatResult<String> {
        let rendered = self.render()?;
        let body = OkfDocument::parse(&rendered)?.body().to_string();
        Ok(remove_html_comments(&body)
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            .split("\n\n\n")
            .collect::<Vec<_>>()
            .join("\n\n")
            .trim()
            .to_string())
    }
}

#[derive(Debug, Clone)]
enum Command {
    Create(KnowledgeEntry),
    Update(String, KnowledgeEntryUpdate),
    Upsert(KnowledgeEntry),
    Replace(KnowledgeEntry),
    Remove(String),
}

/// Chainable facade that applies document commands atomically in memory.
#[derive(Debug, Clone)]
pub struct KnowledgeDocumentBuilder {
    profile: KnowledgeDocumentProfile,
    source: Option<String>,
    commands: Vec<Command>,
}

impl KnowledgeDocumentBuilder {
    /// Creates a canonical empty document.
    #[must_use]
    pub fn create_document(mut self) -> Self {
        self.source = Some(empty_template(&self.profile));
        self
    }

    /// Loads a source document for subsequent commands.
    #[must_use]
    pub fn from_markdown(mut self, content: impl Into<String>) -> Self {
        self.source = Some(content.into());
        self
    }

    /// Replaces the complete source document before applying later commands.
    #[must_use]
    pub fn replace_document(mut self, replacement: impl Into<String>) -> Self {
        self.source = Some(replacement.into());
        self
    }

    #[must_use]
    pub fn create_entry(mut self, entry: KnowledgeEntry) -> Self {
        self.commands.push(Command::Create(entry));
        self
    }

    #[must_use]
    pub fn update_entry(mut self, key: impl Into<String>, update: KnowledgeEntryUpdate) -> Self {
        self.commands.push(Command::Update(key.into(), update));
        self
    }

    #[must_use]
    pub fn upsert_entry(mut self, entry: KnowledgeEntry) -> Self {
        self.commands.push(Command::Upsert(entry));
        self
    }

    #[must_use]
    pub fn replace_entry(mut self, entry: KnowledgeEntry) -> Self {
        self.commands.push(Command::Replace(entry));
        self
    }

    #[must_use]
    pub fn remove_entry(mut self, key: impl Into<String>) -> Self {
        self.commands.push(Command::Remove(key.into()));
        self
    }

    /// Parses, applies all commands, and validates the final immutable result.
    pub fn build(self) -> KnowledgeFormatResult<KnowledgeDocument> {
        self.profile.validate()?;
        let source = self.source.ok_or_else(|| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidDocument,
                "managed document source is required",
            )
        })?;
        let mut document = parse_document(&self.profile, &source)?;
        for command in self.commands {
            apply_command(&mut document, command)?;
        }
        validate_document(&document)?;
        document.render()?;
        Ok(document)
    }
}

fn apply_command(document: &mut KnowledgeDocument, command: Command) -> KnowledgeFormatResult<()> {
    match command {
        Command::Create(entry) => {
            validate_entry(&document.profile, &entry)?;
            if document.get_entry(entry.key.as_str()).is_some() {
                return Err(KnowledgeFormatError::new(
                    KnowledgeFormatErrorKind::AlreadyExists,
                    format!("entry already exists: {}", entry.key.as_str()),
                ));
            }
            insert_in_section_order(document, entry);
        }
        Command::Update(key, update) => {
            validate_key(&key, "entry key", 64)?;
            let entry = document
                .entries
                .iter_mut()
                .find(|entry| entry.key.as_str() == key)
                .ok_or_else(|| {
                    KnowledgeFormatError::new(
                        KnowledgeFormatErrorKind::NotFound,
                        format!("entry not found: {key}"),
                    )
                })?;
            if let Some(section) = update.section {
                entry.section = section;
            }
            if let Some(title) = update.title {
                entry.title = title;
            }
            if let Some(body) = update.body {
                entry.body = body;
            }
            let updated = entry.clone();
            validate_entry(&document.profile, &updated)?;
            sort_entries_by_section(document);
        }
        Command::Upsert(entry) => {
            validate_entry(&document.profile, &entry)?;
            if let Some(existing) = document
                .entries
                .iter_mut()
                .find(|candidate| candidate.key == entry.key)
            {
                *existing = entry;
                sort_entries_by_section(document);
            } else {
                insert_in_section_order(document, entry);
            }
        }
        Command::Replace(entry) => {
            validate_entry(&document.profile, &entry)?;
            let existing = document
                .entries
                .iter_mut()
                .find(|candidate| candidate.key == entry.key)
                .ok_or_else(|| {
                    KnowledgeFormatError::new(
                        KnowledgeFormatErrorKind::NotFound,
                        format!("entry not found: {}", entry.key.as_str()),
                    )
                })?;
            *existing = entry;
            sort_entries_by_section(document);
        }
        Command::Remove(key) => {
            validate_key(&key, "entry key", 64)?;
            document.entries.retain(|entry| entry.key.as_str() != key);
        }
    }
    Ok(())
}

fn validate_document(document: &KnowledgeDocument) -> KnowledgeFormatResult<()> {
    if document.entries.len() > document.profile.limits.max_entries {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::LimitExceeded,
            "managed document entry count exceeds the profile limit",
        ));
    }
    let mut keys = BTreeSet::new();
    for entry in &document.entries {
        validate_entry(&document.profile, entry)?;
        if !keys.insert(entry.key.clone()) {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidDocument,
                format!("duplicate knowledge entry key: {}", entry.key.as_str()),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_entry(
    profile: &KnowledgeDocumentProfile,
    entry: &KnowledgeEntry,
) -> KnowledgeFormatResult<()> {
    if profile.section(&entry.section).is_none() {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidEntry,
            format!("unknown entry section: {}", entry.section.as_str()),
        ));
    }
    validate_key(entry.key.as_str(), "entry key", 64)?;
    validate_single_line(&entry.title, "entry title", 160)?;
    validate_body_headings(entry.body.as_str())?;
    if entry.body.as_str().contains(&profile.region_namespace) {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidEntry,
            "entry body contains a reserved managed document marker",
        ));
    }
    Ok(())
}

pub(super) fn validate_body_headings(body: &str) -> KnowledgeFormatResult<()> {
    if body.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("# ") || line.starts_with("## ") || line.starts_with("### ")
    }) {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidEntry,
            "entry body may only contain level-four or deeper headings",
        ));
    }
    Ok(())
}

fn insert_in_section_order(document: &mut KnowledgeDocument, entry: KnowledgeEntry) {
    let section_position = document
        .profile
        .sections
        .iter()
        .position(|section| section.key == entry.section)
        .unwrap_or(usize::MAX);
    let insert_at = document
        .entries
        .iter()
        .position(|candidate| {
            document
                .profile
                .sections
                .iter()
                .position(|section| section.key == candidate.section)
                .unwrap_or(usize::MAX)
                > section_position
        })
        .unwrap_or(document.entries.len());
    document.entries.insert(insert_at, entry);
}

fn sort_entries_by_section(document: &mut KnowledgeDocument) {
    document.entries.sort_by_key(|entry| {
        document
            .profile
            .sections
            .iter()
            .position(|section| section.key == entry.section)
            .unwrap_or(usize::MAX)
    });
}
