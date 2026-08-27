use crate::OkfFrontmatter;

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
