use std::collections::HashSet;

use sha2::{Digest, Sha256};
use talos_knowledge_format::{OkfDate, OkfIndex, OkfIndexEntry, OkfLog, OkfLogEntry, OkfVersion};

use super::{
    DreamMemoryDate, DreamMemoryEntry, DreamMemoryError, DreamMemoryErrorKind, DreamMemoryResult,
};

const DREAM_MEMORY_CONCEPT_TYPE: &str = "DreamMemory";
const DREAM_MEMORY_LOG_TITLE: &str = "Dream Memory Log";
const MAX_INDEX_TITLES: usize = 3;
const MAX_INDEX_TITLE_CHARS: usize = 48;

/// Immutable summary of one canonical date Concept used to derive bundle materials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryBundleConcept {
    date: DreamMemoryDate,
    revision: DreamMemoryConceptRevision,
    entry_titles: Vec<String>,
}

impl DreamMemoryBundleConcept {
    /// Creates one concept summary from an already validated content revision.
    pub fn new(
        date: DreamMemoryDate,
        revision: DreamMemoryConceptRevision,
        entry_titles: Vec<String>,
    ) -> DreamMemoryResult<Self> {
        if entry_titles.iter().any(|title| {
            title.trim().is_empty()
                || title.trim() != title
                || title.contains(['\r', '\n'])
                || title.chars().any(char::is_control)
        }) {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidInput,
                "Dream Memory bundle entry title is invalid",
            ));
        }
        Ok(Self {
            date,
            revision,
            entry_titles,
        })
    }

    /// Derives the stable bundle representation from canonical Markdown.
    pub fn from_content(
        date: DreamMemoryDate,
        content: &str,
        entries: &[DreamMemoryEntry],
    ) -> DreamMemoryResult<Self> {
        Self::new(
            date,
            DreamMemoryConceptRevision::from_content(content),
            entries
                .iter()
                .map(|entry| entry.title().to_string())
                .collect(),
        )
    }

    #[must_use]
    pub const fn date(&self) -> DreamMemoryDate {
        self.date
    }
}

/// Content-addressed revision of one Dream Memory Concept.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DreamMemoryConceptRevision(String);

impl DreamMemoryConceptRevision {
    /// Parses a canonical lowercase SHA-256 revision.
    pub fn parse(value: impl Into<String>) -> DreamMemoryResult<Self> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidInput,
                "Dream Memory Concept revision is invalid",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn from_content(content: &str) -> Self {
        Self(
            Sha256::digest(content.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Business event recorded when a Concept revision first enters the bundle log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DreamMemoryBundleEventKind {
    Creation,
    Update,
    Recovery,
}

impl DreamMemoryBundleEventKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Creation => "Creation",
            Self::Update => "Update",
            Self::Recovery => "Recovery",
        }
    }
}

/// Complete in-memory derived bundle materials for a Dream Memory directory.
#[derive(Debug, Clone, PartialEq)]
pub struct DreamMemoryBundle {
    index: OkfIndex,
    log: Option<OkfLog>,
}

impl DreamMemoryBundle {
    /// Rebuilds navigation and idempotently records any previously unseen revisions.
    pub fn reconcile(
        concepts: &[DreamMemoryBundleConcept],
        current_log: Option<&str>,
        event_kind: DreamMemoryBundleEventKind,
        logged_on: DreamMemoryDate,
    ) -> DreamMemoryResult<Self> {
        let mut concepts = concepts.to_vec();
        concepts.sort_by_key(|concept| std::cmp::Reverse(concept.date));
        let mut index = OkfIndex::builder().version(OkfVersion::current());
        for concept in &concepts {
            index = index.entry(index_entry(concept));
        }
        let index = index.build()?;
        if concepts.is_empty() {
            return Ok(Self { index, log: None });
        }
        let mut log = match current_log {
            Some(content) => match OkfLog::parse_strict(content) {
                Ok(log) if log.title.as_deref() == Some(DREAM_MEMORY_LOG_TITLE) => Some(log),
                Ok(_) if event_kind == DreamMemoryBundleEventKind::Recovery => {
                    Some(OkfLog::builder().title(DREAM_MEMORY_LOG_TITLE).build()?)
                }
                Ok(_) => {
                    return Err(DreamMemoryError::new(
                        DreamMemoryErrorKind::InvalidDocument,
                        "Dream Memory log title is invalid",
                    ));
                }
                Err(_) if event_kind == DreamMemoryBundleEventKind::Recovery => {
                    Some(OkfLog::builder().title(DREAM_MEMORY_LOG_TITLE).build()?)
                }
                Err(error) => return Err(error.into()),
            },
            None => Some(OkfLog::builder().title(DREAM_MEMORY_LOG_TITLE).build()?),
        };
        let log_date = OkfDate::parse(&logged_on.to_string()).ok_or_else(|| {
            DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidInput,
                "Dream Memory log date is invalid",
            )
        })?;
        for concept in &concepts {
            let text = log_text(concept);
            let log = log.as_mut().ok_or_else(|| {
                DreamMemoryError::new(
                    DreamMemoryErrorKind::InvalidDocument,
                    "Dream Memory log is unavailable for a non-empty bundle",
                )
            })?;
            if !contains_revision(log, &concept.revision) {
                log.append_entry(
                    log_date,
                    OkfLogEntry::new(Some(event_kind.as_str().to_string()), text)?,
                )?;
            }
        }
        Ok(Self { index, log })
    }

    /// Parses the business navigation entries consumed by list APIs.
    pub fn parse_index_dates(content: &str) -> DreamMemoryResult<Vec<DreamMemoryDate>> {
        let index = OkfIndex::parse(content)?;
        if index.version().map(OkfVersion::as_str) != Some(OkfVersion::current().as_str()) {
            return Err(DreamMemoryError::new(
                DreamMemoryErrorKind::InvalidDocument,
                "Dream Memory index must declare the current OKF version",
            ));
        }
        let mut seen = HashSet::new();
        let mut dates = Vec::with_capacity(index.entries.len());
        for entry in index.entries {
            let date_text = entry.link.strip_suffix(".md").ok_or_else(|| {
                DreamMemoryError::new(
                    DreamMemoryErrorKind::InvalidDocument,
                    "Dream Memory index link must target a date Markdown file",
                )
            })?;
            let date = DreamMemoryDate::parse(date_text)?;
            if entry.concept_type != DREAM_MEMORY_CONCEPT_TYPE
                || entry.title != format!("Dream Memory {date}")
                || !seen.insert(date)
            {
                return Err(DreamMemoryError::new(
                    DreamMemoryErrorKind::InvalidDocument,
                    "Dream Memory index contains an invalid or duplicate entry",
                ));
            }
            dates.push(date);
        }
        Ok(dates)
    }

    #[must_use]
    pub fn render_index(&self) -> String {
        self.index.render()
    }

    #[must_use]
    pub fn render_log(&self) -> Option<String> {
        self.log.as_ref().map(OkfLog::render)
    }
}

fn index_entry(concept: &DreamMemoryBundleConcept) -> OkfIndexEntry {
    OkfIndexEntry {
        concept_type: DREAM_MEMORY_CONCEPT_TYPE.to_string(),
        title: format!("Dream Memory {}", concept.date),
        link: format!("{}.md", concept.date),
        description: concept_description(concept),
    }
}

fn log_text(concept: &DreamMemoryBundleConcept) -> String {
    format!(
        "[Dream Memory {}]({}.md) revision `{}`: {}",
        concept.date,
        concept.date,
        concept.revision.as_str(),
        concept_description(concept)
    )
}

fn contains_revision(log: &OkfLog, revision: &DreamMemoryConceptRevision) -> bool {
    let marker = format!("revision `{}`:", revision.as_str());
    log.days
        .iter()
        .flat_map(|day| &day.entries)
        .any(|entry| entry.text.contains(&marker))
}

fn concept_description(concept: &DreamMemoryBundleConcept) -> String {
    let count = concept.entry_titles.len();
    if count == 0 {
        return "Contains no durable memories.".to_string();
    }
    let titles = concept
        .entry_titles
        .iter()
        .take(MAX_INDEX_TITLES)
        .map(|title| abbreviate(title, MAX_INDEX_TITLE_CHARS))
        .collect::<Vec<_>>()
        .join("; ");
    let noun = if count == 1 { "memory" } else { "memories" };
    let remaining = count.saturating_sub(MAX_INDEX_TITLES);
    if remaining == 0 {
        format!("Contains {count} durable {noun}: {titles}.")
    } else {
        format!("Contains {count} durable {noun}: {titles}; and {remaining} more.")
    }
}

fn abbreviate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut abbreviated = value.chars().take(max_chars - 1).collect::<String>();
    abbreviated.push('…');
    abbreviated
}
