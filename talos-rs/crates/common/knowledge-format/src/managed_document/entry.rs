use crate::managed_document::document::validate_body_headings;
use crate::managed_document::profile::{validate_key, validate_single_line};
use crate::{
    EntryKey, KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult,
    MarkdownFragment, SectionKey,
};

/// One stable-key entry inside a managed knowledge region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntry {
    pub key: EntryKey,
    pub section: SectionKey,
    pub title: String,
    pub body: MarkdownFragment,
}

impl KnowledgeEntry {
    /// Starts an entry builder.
    #[must_use]
    pub fn builder() -> KnowledgeEntryBuilder {
        KnowledgeEntryBuilder::default()
    }
}

/// Builder for one structured knowledge entry.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeEntryBuilder {
    key: Option<String>,
    section: Option<String>,
    title: Option<String>,
    body: Option<String>,
}

impl KnowledgeEntryBuilder {
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    #[must_use]
    pub fn section(mut self, section: impl Into<String>) -> Self {
        self.section = Some(section.into());
        self
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn build(self) -> KnowledgeFormatResult<KnowledgeEntry> {
        let key = self.key.unwrap_or_default();
        let section = self.section.unwrap_or_default();
        let title = self.title.unwrap_or_default();
        validate_key(&key, "entry key", 64)?;
        validate_key(&section, "section key", 64)?;
        validate_single_line(&title, "entry title", 160)?;
        let body = self.body.unwrap_or_default();
        validate_body_headings(&body)?;
        Ok(KnowledgeEntry {
            key: EntryKey(key),
            section: SectionKey(section),
            title,
            body: MarkdownFragment(body),
        })
    }
}

/// Field-level update for an existing entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnowledgeEntryUpdate {
    pub section: Option<SectionKey>,
    pub title: Option<String>,
    pub body: Option<MarkdownFragment>,
}

impl KnowledgeEntryUpdate {
    /// Starts an entry update builder.
    #[must_use]
    pub fn builder() -> KnowledgeEntryUpdateBuilder {
        KnowledgeEntryUpdateBuilder::default()
    }
}

/// Builder for a field-level entry update.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeEntryUpdateBuilder(KnowledgeEntryUpdate);

impl KnowledgeEntryUpdateBuilder {
    #[must_use]
    pub fn section(mut self, section: impl Into<String>) -> Self {
        self.0.section = Some(SectionKey(section.into()));
        self
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.0.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.0.body = Some(MarkdownFragment(body.into()));
        self
    }

    pub fn build(self) -> KnowledgeFormatResult<KnowledgeEntryUpdate> {
        if let Some(section) = &self.0.section {
            validate_key(section.as_str(), "section key", 64)?;
        }
        if let Some(title) = &self.0.title {
            validate_single_line(title, "entry title", 160)?;
        }
        if let Some(body) = &self.0.body {
            validate_body_headings(body.as_str())?;
        }
        Ok(self.0)
    }
}

/// Stable query for managed entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnowledgeEntryQuery {
    pub section: Option<SectionKey>,
    pub key_prefix: Option<String>,
    pub limit: Option<usize>,
}

impl KnowledgeEntryQuery {
    /// Query without filters.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            section: None,
            key_prefix: None,
            limit: None,
        }
    }

    /// Starts a query builder.
    #[must_use]
    pub fn builder() -> KnowledgeEntryQueryBuilder {
        KnowledgeEntryQueryBuilder::default()
    }
}

/// Builder for an entry query.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeEntryQueryBuilder(KnowledgeEntryQuery);

impl KnowledgeEntryQueryBuilder {
    #[must_use]
    pub fn section(mut self, section: impl Into<String>) -> Self {
        self.0.section = Some(SectionKey(section.into()));
        self
    }

    #[must_use]
    pub fn key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.0.key_prefix = Some(prefix.into());
        self
    }

    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.0.limit = Some(limit);
        self
    }

    pub fn build(self) -> KnowledgeFormatResult<KnowledgeEntryQuery> {
        if let Some(section) = &self.0.section {
            validate_key(section.as_str(), "section key", 64)?;
        }
        if self.0.limit == Some(0) {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidEntry,
                "entry query limit must be greater than zero",
            ));
        }
        Ok(self.0)
    }
}
