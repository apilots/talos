use crate::{
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult, OkfConcept, OkfDocument,
    OkfVersion,
};

/// One generated or parsed `index.md` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfIndexEntry {
    pub concept_type: String,
    pub title: String,
    pub link: String,
    pub description: String,
}

/// Parsed or generated OKF index document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfIndex {
    version: Option<OkfVersion>,
    pub entries: Vec<OkfIndexEntry>,
}

impl OkfIndex {
    /// Starts a root `index.md` producer.
    #[must_use]
    pub fn builder() -> OkfIndexBuilder {
        OkfIndexBuilder::default()
    }

    /// Parses the standard generated index grouping and link form.
    pub fn parse(content: &str) -> KnowledgeFormatResult<Self> {
        let document = OkfDocument::parse(content)?;
        let frontmatter = &document.inner.frontmatter;
        if frontmatter
            .as_mapping()
            .keys()
            .any(|key| key != "okf_version")
        {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                "root index.md frontmatter may contain only okf_version",
            ));
        }
        let version = match frontmatter.get("okf_version") {
            None => None,
            Some(okf_core::yaml::Value::String(value)) => {
                Some(OkfVersion::parse(value.clone()).ok_or_else(|| {
                    KnowledgeFormatError::new(
                        KnowledgeFormatErrorKind::InvalidOkf,
                        format!("unsupported OKF index version: {value}"),
                    )
                })?)
            }
            Some(_) => {
                return Err(KnowledgeFormatError::new(
                    KnowledgeFormatErrorKind::InvalidOkf,
                    "root index.md okf_version must be a string",
                ));
            }
        };
        let mut concept_type = None;
        let mut entries = Vec::new();
        for line in document.body().lines() {
            if let Some(value) = line.strip_prefix("# ") {
                concept_type = Some(value.trim().to_string());
            } else if let Some(value) = line
                .strip_prefix("- [")
                .or_else(|| line.strip_prefix("* ["))
            {
                let Some((title, rest)) = value.split_once("](") else {
                    continue;
                };
                let Some((link, suffix)) = rest.split_once(')') else {
                    continue;
                };
                entries.push(OkfIndexEntry {
                    concept_type: concept_type.clone().unwrap_or_default(),
                    title: title.to_string(),
                    link: link.to_string(),
                    description: suffix
                        .trim()
                        .strip_prefix('—')
                        .unwrap_or(suffix.trim())
                        .trim()
                        .to_string(),
                });
            }
        }
        Ok(Self { version, entries })
    }

    /// Generates an index from concepts.
    #[must_use]
    pub fn generate(concepts: &[OkfConcept]) -> Self {
        Self {
            version: Some(OkfVersion::current()),
            entries: concepts
                .iter()
                .map(|concept| OkfIndexEntry {
                    concept_type: concept
                        .document
                        .frontmatter()
                        .concept_type()
                        .unwrap_or_default(),
                    title: concept
                        .document
                        .frontmatter()
                        .title()
                        .unwrap_or_else(|| concept.id.as_str().to_string()),
                    link: format!("{}.md", concept.id.as_str()),
                    description: concept
                        .document
                        .frontmatter()
                        .description()
                        .unwrap_or_default(),
                })
                .collect(),
        }
    }

    /// Renders the standard grouped index body.
    #[must_use]
    pub fn render(&self) -> String {
        let body = self.render_body();
        match &self.version {
            Some(version) => format!("---\nokf_version: \"{}\"\n---\n\n{body}", version.as_str()),
            None => body,
        }
    }

    /// Renders only the generated listing body for a nested index.
    #[must_use]
    pub fn render_body(&self) -> String {
        let entries: Vec<_> = self
            .entries
            .iter()
            .map(|entry| okf_core::index::IndexEntry {
                type_: entry.concept_type.clone(),
                title: entry.title.clone(),
                link: entry.link.clone(),
                description: entry.description.clone(),
            })
            .collect();
        okf_core::index::build_index_text(&entries)
    }

    /// Declared root bundle version, when present.
    #[must_use]
    pub fn version(&self) -> Option<&OkfVersion> {
        self.version.as_ref()
    }
}

/// Builder for a deterministic root OKF index.
#[derive(Debug, Clone, Default)]
pub struct OkfIndexBuilder {
    version: Option<OkfVersion>,
    entries: Vec<OkfIndexEntry>,
}

impl OkfIndexBuilder {
    /// Sets the declared bundle version.
    #[must_use]
    pub fn version(mut self, version: OkfVersion) -> Self {
        self.version = Some(version);
        self
    }

    /// Adds one already-typed navigation entry.
    #[must_use]
    pub fn entry(mut self, entry: OkfIndexEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Builds a root index and rejects ambiguous entries.
    pub fn build(self) -> KnowledgeFormatResult<OkfIndex> {
        let mut links = std::collections::HashSet::new();
        if self.entries.iter().any(|entry| {
            entry.concept_type.trim().is_empty()
                || entry.title.trim().is_empty()
                || entry.link.trim().is_empty()
                || entry.title.contains(['\r', '\n'])
                || entry.link.contains(['\r', '\n'])
                || !links.insert(entry.link.clone())
        }) {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                "invalid or duplicate OKF index entry",
            ));
        }
        Ok(OkfIndex {
            version: self.version.or_else(|| Some(OkfVersion::current())),
            entries: self.entries,
        })
    }
}
