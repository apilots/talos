use crate::{KnowledgeFormatResult, OkfConcept, OkfDocument};

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
    pub entries: Vec<OkfIndexEntry>,
}

impl OkfIndex {
    /// Parses the standard generated index grouping and link form.
    pub fn parse(content: &str) -> KnowledgeFormatResult<Self> {
        let document = OkfDocument::parse(content)?;
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
        Ok(Self { entries })
    }

    /// Generates an index from concepts.
    #[must_use]
    pub fn generate(concepts: &[OkfConcept]) -> Self {
        Self {
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
}
