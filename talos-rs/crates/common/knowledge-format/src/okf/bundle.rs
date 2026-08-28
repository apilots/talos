use std::collections::{BTreeMap, BTreeSet};

use crate::{
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult, OkfDiagnosticSeverity,
    OkfDocument, OkfIndex, OkfLog, OkfSource, OkfValidationReport,
};

/// OKF reserved filenames.
pub const OKF_RESERVED_FILENAMES: [&str; 2] = ["index.md", "log.md"];

/// A validated path-derived concept identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OkfConceptId(String);

impl OkfConceptId {
    /// Parses a bundle-relative concept id without a `.md` suffix.
    pub fn parse(value: &str) -> KnowledgeFormatResult<Self> {
        okf_core::ConceptId::parse(value).map_err(|error| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                format!("invalid OKF concept id: {error}"),
            )
        })?;
        Ok(Self(value.to_string()))
    }

    /// Returns the normalized concept id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn core(&self) -> okf_core::ConceptId {
        okf_core::ConceptId::parse(&self.0).expect("validated concept id")
    }
}

/// One in-memory file supplied to an OKF bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfBundleFile {
    pub path: String,
    pub content: String,
}

impl OkfBundleFile {
    /// Creates one in-memory bundle file.
    #[must_use]
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
}

/// One parsed concept in a bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct OkfConcept {
    pub id: OkfConceptId,
    pub path: String,
    pub document: OkfDocument,
}

/// One internal Markdown link resolved against an in-memory bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfResolvedLink {
    pub source: OkfConceptId,
    pub target: OkfConceptId,
    pub exists: bool,
    pub text: String,
    pub raw: String,
}

/// One provenance path resolved against an in-memory bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfResolvedSource {
    pub owner: OkfConceptId,
    pub source: OkfSource,
    pub concept: Option<OkfConceptId>,
}

/// Parse failure retained by permissive bundle consumption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfBundleParseError {
    pub path: String,
    pub message: String,
}

/// Stable, format-level bundle comparison.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OkfBundleDiff {
    pub added: Vec<OkfConceptId>,
    pub removed: Vec<OkfConceptId>,
    pub changed: Vec<OkfConceptId>,
}

/// Complete OKF bundle built only from caller-owned in-memory files.
#[derive(Debug, Clone)]
pub struct OkfBundle {
    files: BTreeMap<String, String>,
    concepts: Vec<OkfConcept>,
    parse_errors: Vec<OkfBundleParseError>,
}

impl OkfBundle {
    /// Starts an in-memory bundle builder.
    #[must_use]
    pub fn builder() -> OkfBundleBuilder {
        OkfBundleBuilder::default()
    }

    /// Parsed, non-reserved concepts.
    #[must_use]
    pub fn concepts(&self) -> &[OkfConcept] {
        &self.concepts
    }

    /// Per-file parse failures retained for permissive consumption.
    #[must_use]
    pub fn parse_errors(&self) -> &[OkfBundleParseError] {
        &self.parse_errors
    }

    /// Looks up one concept by its typed identifier.
    #[must_use]
    pub fn concept(&self, id: &OkfConceptId) -> Option<&OkfConcept> {
        self.concepts.iter().find(|concept| &concept.id == id)
    }

    /// Lists all concepts with the exact producer-defined type.
    pub fn concepts_of_type<'a>(
        &'a self,
        concept_type: &'a str,
    ) -> impl Iterator<Item = &'a OkfConcept> {
        self.concepts.iter().filter(move |concept| {
            concept.document.frontmatter().concept_type().as_deref() == Some(concept_type)
        })
    }

    /// Lists all Attested Computation concepts.
    pub fn attested_computations(&self) -> impl Iterator<Item = &OkfConcept> {
        self.concepts_of_type("Attested Computation")
    }

    /// Synthesizes the consumer-side tag index defined by OKF.
    #[must_use]
    pub fn tags(&self) -> BTreeMap<String, Vec<OkfConceptId>> {
        let mut tags: BTreeMap<String, Vec<OkfConceptId>> = BTreeMap::new();
        for concept in &self.concepts {
            for tag in concept.document.frontmatter().tags() {
                tags.entry(tag).or_default().push(concept.id.clone());
            }
        }
        tags
    }

    /// Returns the declared root `index.md` OKF version when supported.
    #[must_use]
    pub fn okf_version(&self) -> Option<crate::OkfVersion> {
        let content = self.files.get("index.md")?;
        let document = OkfDocument::parse(content).ok()?;
        let value = document.inner.frontmatter.get("okf_version")?;
        crate::OkfVersion::parse(value.as_display_string()?)
    }

    /// Lists concepts stale at an explicit timestamp.
    #[must_use]
    pub fn stale_at(&self, now: crate::OkfDateTime) -> Vec<&OkfConcept> {
        self.concepts
            .iter()
            .filter(|concept| concept.document.is_stale_at(now))
            .collect()
    }

    /// Resolves all internal Markdown links, retaining broken links.
    #[must_use]
    pub fn resolve_links(&self) -> Vec<OkfResolvedLink> {
        let ids: BTreeSet<_> = self
            .concepts
            .iter()
            .map(|concept| concept.id.clone())
            .collect();
        let mut resolved = Vec::new();
        for concept in &self.concepts {
            let source = concept.id.core();
            for link in concept.document.inner.links() {
                for target in link.resolve_all(&source) {
                    let target = OkfConceptId(target.to_string());
                    resolved.push(OkfResolvedLink {
                        source: concept.id.clone(),
                        exists: ids.contains(&target),
                        target,
                        text: link.text.clone(),
                        raw: link.target.clone(),
                    });
                }
            }
        }
        resolved
    }

    /// Resolves path-valued provenance resources to concepts when possible.
    #[must_use]
    pub fn resolve_sources(&self) -> Vec<OkfResolvedSource> {
        let ids: BTreeSet<_> = self
            .concepts
            .iter()
            .map(|concept| concept.id.clone())
            .collect();
        self.concepts
            .iter()
            .flat_map(|concept| {
                concept.document.sources().into_iter().map(|source| {
                    let target = source
                        .resource
                        .as_deref()
                        .and_then(concept_id_from_path)
                        .filter(|id| ids.contains(id));
                    OkfResolvedSource {
                        owner: concept.id.clone(),
                        source,
                        concept: target,
                    }
                })
            })
            .collect()
    }

    /// Returns concepts that link to the supplied target.
    #[must_use]
    pub fn backlinks(&self, target: &OkfConceptId) -> Vec<OkfConceptId> {
        self.resolve_links()
            .into_iter()
            .filter(|link| &link.target == target)
            .map(|link| link.source)
            .collect()
    }

    /// Returns typed provenance derivation targets for one concept.
    #[must_use]
    pub fn derived_from(&self, owner: &OkfConceptId) -> Vec<OkfConceptId> {
        self.resolve_sources()
            .into_iter()
            .filter(|source| &source.owner == owner)
            .filter_map(|source| source.concept)
            .collect()
    }

    /// Returns concepts whose provenance derives from the supplied concept.
    #[must_use]
    pub fn derives(&self, source: &OkfConceptId) -> Vec<OkfConceptId> {
        self.resolve_sources()
            .into_iter()
            .filter(|resolved| resolved.concept.as_ref() == Some(source))
            .map(|resolved| resolved.owner)
            .collect()
    }

    /// Validates concepts, reserved files, declared version, and link graph.
    #[must_use]
    pub fn validate(&self) -> OkfValidationReport {
        let mut report = OkfValidationReport::default();
        for error in &self.parse_errors {
            report.push(
                OkfDiagnosticSeverity::Error,
                Some(error.path.clone()),
                None,
                error.message.clone(),
            );
        }
        for concept in &self.concepts {
            for diagnostic in concept.document.validate().diagnostics {
                report.push_fixable(
                    diagnostic.severity,
                    Some(concept.path.clone()),
                    Some(concept.id.as_str().to_string()),
                    diagnostic.message,
                    diagnostic.fixable,
                );
            }
        }
        for link in self.resolve_links().into_iter().filter(|link| !link.exists) {
            report.push(
                OkfDiagnosticSeverity::Info,
                None,
                Some(link.source.as_str().to_string()),
                format!("broken link is permitted: {}", link.raw),
            );
        }
        for (path, content) in &self.files {
            if path.ends_with("log.md") {
                let log = OkfLog::parse(content);
                for message in log.structural_errors(content) {
                    report.push(
                        OkfDiagnosticSeverity::Error,
                        Some(path.clone()),
                        None,
                        message,
                    );
                }
            }
            if path.ends_with("index.md") {
                if let Err(error) = OkfIndex::parse(content) {
                    report.push(
                        OkfDiagnosticSeverity::Error,
                        Some(path.clone()),
                        None,
                        error.to_string(),
                    );
                }
            }
        }
        report
    }

    /// Compares concept presence and canonical document content.
    #[must_use]
    pub fn diff(&self, other: &Self) -> OkfBundleDiff {
        let left: BTreeMap<_, _> = self
            .concepts
            .iter()
            .map(|concept| (concept.id.clone(), concept.document.render()))
            .collect();
        let right: BTreeMap<_, _> = other
            .concepts
            .iter()
            .map(|concept| (concept.id.clone(), concept.document.render()))
            .collect();
        OkfBundleDiff {
            added: right
                .keys()
                .filter(|id| !left.contains_key(*id))
                .cloned()
                .collect(),
            removed: left
                .keys()
                .filter(|id| !right.contains_key(*id))
                .cloned()
                .collect(),
            changed: left
                .iter()
                .filter_map(|(id, content)| {
                    right
                        .get(id)
                        .is_some_and(|other| other != content)
                        .then_some(id.clone())
                })
                .collect(),
        }
    }
}

/// Builder that validates bundle-relative paths without touching the filesystem.
#[derive(Debug, Clone, Default)]
pub struct OkfBundleBuilder {
    files: Vec<OkfBundleFile>,
}

impl OkfBundleBuilder {
    /// Adds one caller-owned in-memory file.
    #[must_use]
    pub fn file(mut self, file: OkfBundleFile) -> Self {
        self.files.push(file);
        self
    }

    /// Builds a permissive in-memory bundle.
    pub fn build(self) -> KnowledgeFormatResult<OkfBundle> {
        let mut files = BTreeMap::new();
        for file in self.files {
            validate_bundle_path(&file.path)?;
            if files.insert(file.path.clone(), file.content).is_some() {
                return Err(KnowledgeFormatError::new(
                    KnowledgeFormatErrorKind::InvalidOkf,
                    format!("duplicate bundle path: {}", file.path),
                ));
            }
        }
        let mut concepts = Vec::new();
        let mut parse_errors = Vec::new();
        for (path, content) in &files {
            if !path.ends_with(".md") || is_reserved(path) {
                continue;
            }
            let id_text = path.trim_end_matches(".md");
            let id = OkfConceptId::parse(id_text)?;
            match OkfDocument::parse(content) {
                Ok(document) => concepts.push(OkfConcept {
                    id,
                    path: path.clone(),
                    document,
                }),
                Err(error) => parse_errors.push(OkfBundleParseError {
                    path: path.clone(),
                    message: error.to_string(),
                }),
            }
        }
        Ok(OkfBundle {
            files,
            concepts,
            parse_errors,
        })
    }
}

fn validate_bundle_path(path: &str) -> KnowledgeFormatResult<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidOkf,
            format!("bundle path is not portable: {path}"),
        ));
    }
    Ok(())
}

fn is_reserved(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|file| OKF_RESERVED_FILENAMES.contains(&file))
}

fn concept_id_from_path(path: &str) -> Option<OkfConceptId> {
    if path.contains("://") || path.starts_with("mailto:") {
        return None;
    }
    let normalized = path.trim_start_matches('/').trim_end_matches(".md");
    OkfConceptId::parse(normalized).ok()
}
