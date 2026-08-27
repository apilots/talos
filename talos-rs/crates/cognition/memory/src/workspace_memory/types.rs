use serde::{Deserialize, Serialize};
use talos_knowledge_format::{
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult,
};
use talos_types::{ExpectedFileVersion, MemoryKind};

/// Canonical sections in User Workspace Memory.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UserWorkspaceMemorySection {
    InteractionGuidance,
    KeyKnowledge,
    ActiveContext,
}

impl UserWorkspaceMemorySection {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::InteractionGuidance => "interaction-guidance",
            Self::KeyKnowledge => "key-knowledge",
            Self::ActiveContext => "active-context",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "interaction-guidance" => Some(Self::InteractionGuidance),
            "key-knowledge" => Some(Self::KeyKnowledge),
            "active-context" => Some(Self::ActiveContext),
            _ => None,
        }
    }
}

/// One durable business entry stored in User Workspace Memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserWorkspaceMemoryEntry {
    pub key: String,
    pub section: UserWorkspaceMemorySection,
    pub title: String,
    pub kind: MemoryKind,
    pub summary: String,
    pub why: Option<String>,
    pub apply: Option<String>,
    pub status: Option<String>,
    pub next: Option<String>,
}

impl UserWorkspaceMemoryEntry {
    /// Starts a typed entry builder.
    #[must_use]
    pub fn builder() -> UserWorkspaceMemoryEntryBuilder {
        UserWorkspaceMemoryEntryBuilder::default()
    }

    /// Validates all business fields before rendering or persistence.
    pub fn validate(&self) -> KnowledgeFormatResult<()> {
        validate_memory_key(&self.key, "entry key")?;
        validate_memory_line(&self.title, "entry title", 160)?;
        validate_memory_line(&self.summary, "entry summary", 2_000)?;
        validate_optional_memory_line(self.why.as_deref(), "why", 2_000)?;
        validate_optional_memory_line(self.apply.as_deref(), "apply", 2_000)?;
        validate_optional_memory_line(self.status.as_deref(), "status", 512)?;
        validate_optional_memory_line(self.next.as_deref(), "next", 2_000)
    }
}

/// Builder for a typed User Workspace Memory entry.
#[derive(Debug, Clone, Default)]
pub struct UserWorkspaceMemoryEntryBuilder {
    key: Option<String>,
    section: Option<UserWorkspaceMemorySection>,
    title: Option<String>,
    kind: Option<MemoryKind>,
    summary: Option<String>,
    why: Option<String>,
    apply: Option<String>,
    status: Option<String>,
    next: Option<String>,
}

macro_rules! string_setter {
    ($name:ident, $field:ident) => {
        #[must_use]
        pub fn $name(mut self, value: impl Into<String>) -> Self {
            self.$field = Some(value.into());
            self
        }
    };
}

impl UserWorkspaceMemoryEntryBuilder {
    string_setter!(key, key);
    string_setter!(title, title);
    string_setter!(summary, summary);
    string_setter!(why, why);
    string_setter!(apply, apply);
    string_setter!(status, status);
    string_setter!(next, next);

    #[must_use]
    pub fn section(mut self, section: UserWorkspaceMemorySection) -> Self {
        self.section = Some(section);
        self
    }

    #[must_use]
    pub fn kind(mut self, kind: MemoryKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn build(self) -> KnowledgeFormatResult<UserWorkspaceMemoryEntry> {
        let entry = UserWorkspaceMemoryEntry {
            key: required(self.key, "entry key")?,
            section: self.section.ok_or_else(|| missing("entry section"))?,
            title: required(self.title, "entry title")?,
            kind: self.kind.ok_or_else(|| missing("entry kind"))?,
            summary: required(self.summary, "entry summary")?,
            why: non_empty_optional(self.why, "why")?,
            apply: non_empty_optional(self.apply, "apply")?,
            status: non_empty_optional(self.status, "status")?,
            next: non_empty_optional(self.next, "next")?,
        };
        entry.validate()?;
        Ok(entry)
    }
}

/// Three-state update for optional entry fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "action",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OptionalField<T> {
    #[default]
    Unchanged,
    Set(T),
    Clear,
}

/// Field-level update of an existing typed entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserWorkspaceMemoryEntryUpdate {
    pub section: Option<UserWorkspaceMemorySection>,
    pub title: Option<String>,
    pub kind: Option<MemoryKind>,
    pub summary: Option<String>,
    #[serde(default)]
    pub why: OptionalField<String>,
    #[serde(default)]
    pub apply: OptionalField<String>,
    #[serde(default)]
    pub status: OptionalField<String>,
    #[serde(default)]
    pub next: OptionalField<String>,
}

impl UserWorkspaceMemoryEntryUpdate {
    /// Starts a typed update builder.
    #[must_use]
    pub fn builder() -> UserWorkspaceMemoryEntryUpdateBuilder {
        UserWorkspaceMemoryEntryUpdateBuilder::default()
    }

    /// Validates every changed field before applying the update.
    pub fn validate(&self) -> KnowledgeFormatResult<()> {
        if let Some(title) = &self.title {
            validate_memory_line(title, "entry title", 160)?;
        }
        if let Some(summary) = &self.summary {
            validate_memory_line(summary, "entry summary", 2_000)?;
        }
        validate_optional_update(&self.why, "why", 2_000)?;
        validate_optional_update(&self.apply, "apply", 2_000)?;
        validate_optional_update(&self.status, "status", 512)?;
        validate_optional_update(&self.next, "next", 2_000)
    }
}

/// Builder for a typed field-level entry update.
#[derive(Debug, Clone, Default)]
pub struct UserWorkspaceMemoryEntryUpdateBuilder(UserWorkspaceMemoryEntryUpdate);

impl UserWorkspaceMemoryEntryUpdateBuilder {
    #[must_use]
    pub fn section(mut self, value: UserWorkspaceMemorySection) -> Self {
        self.0.section = Some(value);
        self
    }

    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.0.title = Some(value.into());
        self
    }

    #[must_use]
    pub fn kind(mut self, value: MemoryKind) -> Self {
        self.0.kind = Some(value);
        self
    }

    #[must_use]
    pub fn summary(mut self, value: impl Into<String>) -> Self {
        self.0.summary = Some(value.into());
        self
    }

    #[must_use]
    pub fn why(mut self, value: OptionalField<String>) -> Self {
        self.0.why = value;
        self
    }

    #[must_use]
    pub fn apply(mut self, value: OptionalField<String>) -> Self {
        self.0.apply = value;
        self
    }

    #[must_use]
    pub fn status(mut self, value: OptionalField<String>) -> Self {
        self.0.status = value;
        self
    }

    #[must_use]
    pub fn next(mut self, value: OptionalField<String>) -> Self {
        self.0.next = value;
        self
    }

    pub fn build(self) -> KnowledgeFormatResult<UserWorkspaceMemoryEntryUpdate> {
        self.0.validate()?;
        Ok(self.0)
    }
}

/// Typed structured query for User Workspace Memory.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserWorkspaceMemoryEntryQuery {
    pub section: Option<UserWorkspaceMemorySection>,
    pub key_prefix: Option<String>,
    pub limit: Option<usize>,
}

impl UserWorkspaceMemoryEntryQuery {
    /// Starts a query builder.
    #[must_use]
    pub fn builder() -> UserWorkspaceMemoryEntryQueryBuilder {
        UserWorkspaceMemoryEntryQueryBuilder::default()
    }
}

/// Builder for structured memory queries.
#[derive(Debug, Clone, Default)]
pub struct UserWorkspaceMemoryEntryQueryBuilder(UserWorkspaceMemoryEntryQuery);

impl UserWorkspaceMemoryEntryQueryBuilder {
    #[must_use]
    pub fn section(mut self, value: UserWorkspaceMemorySection) -> Self {
        self.0.section = Some(value);
        self
    }

    #[must_use]
    pub fn key_prefix(mut self, value: impl Into<String>) -> Self {
        self.0.key_prefix = Some(value.into());
        self
    }

    #[must_use]
    pub fn limit(mut self, value: usize) -> Self {
        self.0.limit = Some(value);
        self
    }

    pub fn build(self) -> KnowledgeFormatResult<UserWorkspaceMemoryEntryQuery> {
        if self.0.limit == Some(0) {
            return Err(missing("query limit"));
        }
        Ok(self.0)
    }
}

/// Complete update language used by the Tool and Writer boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserWorkspaceMemoryUpdate {
    CreateEntry {
        entry: UserWorkspaceMemoryEntry,
    },
    UpdateEntry {
        key: String,
        update: UserWorkspaceMemoryEntryUpdate,
    },
    UpsertEntry {
        entry: UserWorkspaceMemoryEntry,
    },
    ReplaceEntry {
        entry: UserWorkspaceMemoryEntry,
    },
    RemoveEntry {
        key: String,
    },
    ReplaceDocument {
        content: String,
        base_version: ExpectedFileVersion,
    },
}

impl UserWorkspaceMemoryUpdate {
    /// Validates the complete structured mutation before it reaches a writer.
    pub fn validate(&self) -> KnowledgeFormatResult<()> {
        match self {
            Self::CreateEntry { entry }
            | Self::UpsertEntry { entry }
            | Self::ReplaceEntry { entry } => entry.validate(),
            Self::UpdateEntry { key, update } => {
                validate_memory_key(key, "entry key")?;
                update.validate()
            }
            Self::RemoveEntry { key } => validate_memory_key(key, "entry key"),
            Self::ReplaceDocument { content, .. } => {
                if content.chars().count() > talos_types::MEMORY_FILE_MAX_CHARS {
                    return Err(KnowledgeFormatError::new(
                        KnowledgeFormatErrorKind::LimitExceeded,
                        "User Workspace Memory document exceeds the character limit",
                    ));
                }
                Ok(())
            }
        }
    }
}

fn required(value: Option<String>, label: &str) -> KnowledgeFormatResult<String> {
    let value = value.unwrap_or_default();
    if value.trim().is_empty() {
        Err(missing(label))
    } else {
        Ok(value)
    }
}

fn non_empty_optional(value: Option<String>, label: &str) -> KnowledgeFormatResult<Option<String>> {
    match value {
        Some(value) if value.trim().is_empty() => Err(missing(label)),
        other => Ok(other),
    }
}

fn missing(label: &str) -> KnowledgeFormatError {
    KnowledgeFormatError::new(
        KnowledgeFormatErrorKind::InvalidEntry,
        format!("{label} must be non-empty"),
    )
}

fn validate_memory_line(value: &str, label: &str, max_chars: usize) -> KnowledgeFormatResult<()> {
    if value.trim().is_empty()
        || value.chars().count() > max_chars
        || value.contains(['\n', '\r'])
        || value.contains("<!--")
        || value.chars().any(char::is_control)
    {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidEntry,
            format!("{label} must be a non-empty single-line value"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_memory_key(value: &str, label: &str) -> KnowledgeFormatResult<()> {
    let mut characters = value.chars();
    if value.len() > 64
        || !characters
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-'
        })
    {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidEntry,
            format!("invalid {label}"),
        ));
    }
    Ok(())
}

fn validate_optional_memory_line(
    value: Option<&str>,
    label: &str,
    max_chars: usize,
) -> KnowledgeFormatResult<()> {
    match value {
        Some(value) => validate_memory_line(value, label, max_chars),
        None => Ok(()),
    }
}

fn validate_optional_update(
    value: &OptionalField<String>,
    label: &str,
    max_chars: usize,
) -> KnowledgeFormatResult<()> {
    match value {
        OptionalField::Set(value) => validate_memory_line(value, label, max_chars),
        OptionalField::Unchanged | OptionalField::Clear => Ok(()),
    }
}
