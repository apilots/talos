use crate::{
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult, OkfFrontmatter,
};

/// One stable managed document section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSectionDefinition {
    pub key: SectionKey,
    pub title: String,
    pub description: Option<String>,
}

/// Hard limits applied to every builder input and render result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnowledgeDocumentLimits {
    pub max_chars: usize,
    pub max_entries: usize,
}

impl Default for KnowledgeDocumentLimits {
    fn default() -> Self {
        Self {
            max_chars: 10_000,
            max_entries: 256,
        }
    }
}

/// Complete definition of one Talos managed knowledge document family.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeDocumentProfile {
    pub(crate) frontmatter: OkfFrontmatter,
    pub(crate) region_namespace: String,
    pub(crate) sections: Vec<KnowledgeSectionDefinition>,
    pub(crate) limits: KnowledgeDocumentLimits,
    pub(crate) guidance: Option<String>,
}

impl KnowledgeDocumentProfile {
    /// Starts a profile builder.
    #[must_use]
    pub fn builder() -> KnowledgeDocumentProfileBuilder {
        KnowledgeDocumentProfileBuilder::default()
    }

    /// Canonical OKF frontmatter owned by this document family.
    #[must_use]
    pub fn frontmatter(&self) -> &OkfFrontmatter {
        &self.frontmatter
    }

    /// Reserved marker namespace owned by this document family.
    #[must_use]
    pub fn region_namespace(&self) -> &str {
        &self.region_namespace
    }

    /// Sections in canonical render order.
    #[must_use]
    pub fn sections(&self) -> &[KnowledgeSectionDefinition] {
        &self.sections
    }

    /// Hard document limits.
    #[must_use]
    pub const fn limits(&self) -> KnowledgeDocumentLimits {
        self.limits
    }

    /// Template-only guidance, when configured.
    #[must_use]
    pub fn guidance(&self) -> Option<&str> {
        self.guidance.as_deref()
    }

    pub(crate) fn validate(&self) -> KnowledgeFormatResult<()> {
        validate_profile(
            &self.frontmatter,
            &self.region_namespace,
            &self.sections,
            self.limits,
        )
    }

    pub(crate) fn section(&self, key: &SectionKey) -> Option<&KnowledgeSectionDefinition> {
        self.sections.iter().find(|section| &section.key == key)
    }

    pub(crate) fn start_marker(&self) -> String {
        format!("<!-- {}:start -->", self.region_namespace)
    }

    pub(crate) fn end_marker(&self) -> String {
        format!("<!-- {}:end -->", self.region_namespace)
    }

    pub(crate) fn entry_marker_prefix(&self) -> String {
        format!("<!-- {}:entry key=\"", self.region_namespace)
    }
}

/// Builder for a managed document profile.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeDocumentProfileBuilder {
    frontmatter: Option<OkfFrontmatter>,
    region_namespace: Option<String>,
    sections: Vec<KnowledgeSectionDefinition>,
    limits: KnowledgeDocumentLimits,
    guidance: Option<String>,
}

impl KnowledgeDocumentProfileBuilder {
    /// Sets complete default OKF frontmatter.
    #[must_use]
    pub fn frontmatter(mut self, frontmatter: OkfFrontmatter) -> Self {
        self.frontmatter = Some(frontmatter);
        self
    }

    /// Sets the reserved marker namespace.
    #[must_use]
    pub fn region_namespace(mut self, value: impl Into<String>) -> Self {
        self.region_namespace = Some(value.into());
        self
    }

    /// Adds a section in canonical render order.
    #[must_use]
    pub fn section(mut self, key: impl Into<String>, title: impl Into<String>) -> Self {
        self.sections.push(KnowledgeSectionDefinition {
            key: SectionKey(key.into()),
            title: title.into(),
            description: None,
        });
        self
    }

    /// Adds a section with a template-only guidance comment.
    #[must_use]
    pub fn section_with_description(
        mut self,
        key: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.sections.push(KnowledgeSectionDefinition {
            key: SectionKey(key.into()),
            title: title.into(),
            description: Some(description.into()),
        });
        self
    }

    /// Sets a template-only document guidance comment.
    #[must_use]
    pub fn guidance(mut self, guidance: impl Into<String>) -> Self {
        self.guidance = Some(guidance.into());
        self
    }

    /// Sets the rendered character cap.
    #[must_use]
    pub fn max_chars(mut self, max_chars: usize) -> Self {
        self.limits.max_chars = max_chars;
        self
    }

    /// Sets the entry count cap.
    #[must_use]
    pub fn max_entries(mut self, max_entries: usize) -> Self {
        self.limits.max_entries = max_entries;
        self
    }

    /// Validates and builds the profile.
    pub fn build(self) -> KnowledgeFormatResult<KnowledgeDocumentProfile> {
        let frontmatter = self.frontmatter.ok_or_else(|| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidProfile,
                "managed document frontmatter is required",
            )
        })?;
        let namespace = self.region_namespace.unwrap_or_default();
        let profile = KnowledgeDocumentProfile {
            frontmatter,
            region_namespace: namespace,
            sections: self.sections,
            limits: self.limits,
            guidance: self.guidance,
        };
        profile.validate()?;
        Ok(profile)
    }
}

fn validate_profile(
    frontmatter: &OkfFrontmatter,
    namespace: &str,
    sections: &[KnowledgeSectionDefinition],
    limits: KnowledgeDocumentLimits,
) -> KnowledgeFormatResult<()> {
    if namespace.is_empty()
        || namespace.chars().any(char::is_whitespace)
        || namespace.contains("-->")
    {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidProfile,
            "managed document region namespace is invalid",
        ));
    }
    if sections.is_empty() || limits.max_chars == 0 || limits.max_entries == 0 {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidProfile,
            "managed document sections and limits must be non-empty",
        ));
    }
    let document = okf_core::Document::new(frontmatter.inner.clone(), "");
    document.validate().map_err(|error| {
        KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidProfile,
            format!("managed document frontmatter is invalid: {error}"),
        )
    })?;
    let mut seen = std::collections::BTreeSet::new();
    for section in sections {
        validate_key(&section.key.0, "section", 64)?;
        validate_single_line(&section.title, "section title", 160)?;
        if !seen.insert(section.key.clone()) {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidProfile,
                format!("duplicate managed document section: {}", section.key.0),
            ));
        }
    }
    Ok(())
}

/// Stable entry key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryKey(pub(crate) String);

impl EntryKey {
    /// Returns the key string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable section key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionKey(pub(crate) String);

impl SectionKey {
    /// Returns the key string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated Markdown body fragment owned by one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownFragment(pub(crate) String);

impl MarkdownFragment {
    /// Returns Markdown text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn validate_key(value: &str, label: &str, max: usize) -> KnowledgeFormatResult<()> {
    let mut chars = value.chars();
    let valid_first = chars
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric());
    if !valid_first
        || value.len() > max
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-'
        })
    {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidEntry,
            format!("invalid {label}: {value}"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_single_line(
    value: &str,
    label: &str,
    max: usize,
) -> KnowledgeFormatResult<()> {
    if value.is_empty()
        || value.chars().count() > max
        || value.contains('\n')
        || value.contains('\r')
        || value.contains("<!--")
        || value.chars().any(char::is_control)
    {
        return Err(KnowledgeFormatError::new(
            KnowledgeFormatErrorKind::InvalidEntry,
            format!("invalid {label}"),
        ));
    }
    Ok(())
}
