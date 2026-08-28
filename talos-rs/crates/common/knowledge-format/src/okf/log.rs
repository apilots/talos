use crate::{
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult, OkfDate, OkfFrontmatter,
};

/// One OKF log entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfLogEntry {
    pub kind: Option<String>,
    pub text: String,
}

/// One date group in an OKF log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfLogDay {
    pub date: String,
    pub entries: Vec<OkfLogEntry>,
}

/// Parsed OKF `log.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct OkfLog {
    pub frontmatter: OkfFrontmatter,
    pub title: Option<String>,
    pub days: Vec<OkfLogDay>,
}

impl OkfLog {
    /// Starts a strongly typed `log.md` producer.
    #[must_use]
    pub fn builder() -> OkfLogBuilder {
        OkfLogBuilder::default()
    }

    /// Permissively parses an OKF log.
    #[must_use]
    pub fn parse(content: &str) -> Self {
        let log = okf_core::Log::parse(content);
        Self {
            frontmatter: OkfFrontmatter {
                inner: log.frontmatter,
            },
            title: log.title,
            days: log
                .days
                .into_iter()
                .map(|day| OkfLogDay {
                    date: day.date,
                    entries: day
                        .entries
                        .into_iter()
                        .map(|entry| OkfLogEntry {
                            kind: entry.kind,
                            text: entry.text,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Renders an OKF log.
    #[must_use]
    pub fn render(&self) -> String {
        self.core_log().to_markdown()
    }

    /// Parses and rejects structural violations that the permissive consumer retains.
    pub fn parse_strict(content: &str) -> KnowledgeFormatResult<Self> {
        let log = Self::parse(content);
        if let Some(date) = log.core_log().invalid_dates().first() {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                format!("invalid OKF log date: {date}"),
            ));
        }
        let errors = log.structural_errors(content);
        if let Some(error) = errors.first() {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                error.clone(),
            ));
        }
        Ok(log)
    }

    /// Returns whether any date group already records the exact semantic text.
    #[must_use]
    pub fn contains_text(&self, text: &str) -> bool {
        self.days
            .iter()
            .flat_map(|day| &day.entries)
            .any(|entry| entry.text == text)
    }

    /// Idempotently appends an entry and keeps ISO date groups newest first.
    pub fn append_entry(
        &mut self,
        date: OkfDate,
        entry: OkfLogEntry,
    ) -> KnowledgeFormatResult<bool> {
        validate_log_entry(&entry)?;
        if self
            .days
            .iter()
            .flat_map(|day| &day.entries)
            .any(|existing| existing == &entry)
        {
            return Ok(false);
        }
        let date = date.to_string();
        match self.days.iter_mut().find(|day| day.date == date) {
            Some(day) => day.entries.push(entry),
            None => self.days.push(OkfLogDay {
                date,
                entries: vec![entry],
            }),
        }
        self.days.sort_by(|left, right| right.date.cmp(&left.date));
        Ok(true)
    }

    pub(crate) fn structural_errors(&self, content: &str) -> Vec<String> {
        self.core_log().structural_errors(content)
    }

    fn core_log(&self) -> okf_core::Log {
        okf_core::Log {
            frontmatter: self.frontmatter.inner.clone(),
            title: self.title.clone(),
            days: self
                .days
                .iter()
                .map(|day| okf_core::log::LogDay {
                    date: day.date.clone(),
                    entries: day
                        .entries
                        .iter()
                        .map(|entry| okf_core::log::LogEntry {
                            kind: entry.kind.clone(),
                            text: entry.text.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

impl OkfLogEntry {
    /// Creates one validated producer log entry.
    pub fn new(kind: Option<String>, text: impl Into<String>) -> KnowledgeFormatResult<Self> {
        let entry = Self {
            kind,
            text: text.into(),
        };
        validate_log_entry(&entry)?;
        Ok(entry)
    }
}

/// Builder for a canonical OKF update log.
#[derive(Debug, Clone, Default)]
pub struct OkfLogBuilder {
    frontmatter: Option<OkfFrontmatter>,
    title: Option<String>,
}

impl OkfLogBuilder {
    /// Sets optional reserved-log frontmatter.
    #[must_use]
    pub fn frontmatter(mut self, frontmatter: OkfFrontmatter) -> Self {
        self.frontmatter = Some(frontmatter);
        self
    }

    /// Sets the single top-level log title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Builds an empty producer log ready for its first append.
    pub fn build(self) -> KnowledgeFormatResult<OkfLog> {
        let title = self.title.ok_or_else(|| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                "OKF log title is required",
            )
        })?;
        if title.trim().is_empty() || title.contains(['\r', '\n']) {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                "OKF log title must be one non-empty line",
            ));
        }
        Ok(OkfLog {
            frontmatter: self.frontmatter.unwrap_or_default(),
            title: Some(title),
            days: Vec::new(),
        })
    }
}

fn validate_log_entry(entry: &OkfLogEntry) -> KnowledgeFormatResult<()> {
    if entry.text.trim().is_empty()
        || entry.text.contains(['\r', '\n'])
        || entry
            .kind
            .as_ref()
            .is_some_and(|kind| kind.trim().is_empty() || kind.contains(['\r', '\n']))
    {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidOkf,
            "OKF log entry must contain single-line kind and text",
        ));
    }
    Ok(())
}
