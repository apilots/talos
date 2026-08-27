use crate::{
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult, OkfAttestedComputation,
    OkfAttester, OkfAttribution, OkfComputationSource, OkfDiagnosticSeverity, OkfExecutor,
    OkfFrontmatter, OkfGenerated, OkfInlineComputation, OkfLink, OkfParameter, OkfSource,
    OkfStatus, OkfTrustTier, OkfValidationReport, OkfVerification,
};

/// Complete in-memory OKF concept document facade.
#[derive(Debug, Clone, PartialEq)]
pub struct OkfDocument {
    pub(crate) inner: okf_core::Document,
}

impl OkfDocument {
    /// Starts an OKF document builder.
    #[must_use]
    pub fn builder() -> OkfDocumentBuilder {
        OkfDocumentBuilder::default()
    }

    /// Parses an OKF concept while preserving unknown frontmatter fields.
    pub fn parse(content: &str) -> KnowledgeFormatResult<Self> {
        let inner = okf_core::Document::parse(content).map_err(|error| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                format!("parse OKF document: {error}"),
            )
        })?;
        Ok(Self { inner })
    }

    /// Returns frontmatter through the stable Talos facade.
    #[must_use]
    pub fn frontmatter(&self) -> OkfFrontmatter {
        OkfFrontmatter {
            inner: self.inner.frontmatter.clone(),
        }
    }

    /// Returns the Markdown body.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.inner.body
    }

    /// Performs deterministic concept-level conformance checks.
    #[must_use]
    pub fn validate(&self) -> OkfValidationReport {
        let mut report = OkfValidationReport::default();
        if let Err(error) = self.inner.validate() {
            report.push(OkfDiagnosticSeverity::Error, None, None, error.to_string());
        }
        super::conformance::validate_document_families(&self.inner, &mut report);
        report
    }

    /// Recommended producer fields that are absent.
    #[must_use]
    pub fn missing_recommended(&self) -> Vec<String> {
        self.inner
            .missing_recommended()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// Renders canonical OKF Markdown.
    #[must_use]
    pub fn render(&self) -> String {
        self.inner.serialize()
    }

    /// Markdown cross-links.
    #[must_use]
    pub fn links(&self) -> Vec<OkfLink> {
        self.inner
            .links()
            .into_iter()
            .map(|link| OkfLink {
                text: link.text,
                target: link.target,
                kind: match link.kind {
                    okf_core::links::LinkKind::Absolute => crate::OkfLinkKind::Absolute,
                    okf_core::links::LinkKind::Relative => crate::OkfLinkKind::Relative,
                    okf_core::links::LinkKind::External => crate::OkfLinkKind::External,
                    okf_core::links::LinkKind::Anchor => crate::OkfLinkKind::Anchor,
                    okf_core::links::LinkKind::Other => crate::OkfLinkKind::Other,
                },
            })
            .collect()
    }

    /// Provenance sources.
    #[must_use]
    pub fn sources(&self) -> Vec<OkfSource> {
        self.inner
            .frontmatter
            .sources()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    /// Body footnotes joined to provenance sources.
    #[must_use]
    pub fn attributions(&self) -> Vec<OkfAttribution> {
        self.inner
            .attributions()
            .into_iter()
            .map(|attribution| OkfAttribution {
                label: attribution.label,
                source: attribution.source.map(Into::into),
                references: attribution.references,
                definitions: attribution.definitions,
            })
            .collect()
    }

    /// Producer signal.
    #[must_use]
    pub fn generated(&self) -> Option<OkfGenerated> {
        self.inner
            .frontmatter
            .generated()
            .map(|generated| OkfGenerated {
                by: generated.by.map(Into::into),
                at: generated
                    .at
                    .map(|value| crate::OkfDateTimeValue::from_raw(value.raw)),
            })
    }

    /// Verification events, including normalization of a bare mapping.
    #[must_use]
    pub fn verifications(&self) -> Vec<OkfVerification> {
        self.inner
            .frontmatter
            .verified()
            .into_iter()
            .map(|verification| {
                let valid = verification.is_valid();
                OkfVerification {
                    by: verification.by.map(Into::into),
                    at: verification
                        .at
                        .map(|value| crate::OkfDateTimeValue::from_raw(value.raw)),
                    valid,
                }
            })
            .collect()
    }

    /// Derived trust tier.
    #[must_use]
    pub fn trust_tier(&self) -> OkfTrustTier {
        self.inner.frontmatter.trust_tier().into()
    }

    /// Lifecycle status.
    #[must_use]
    pub fn status(&self) -> OkfStatus {
        self.inner.frontmatter.status().into()
    }

    /// Evaluates `stale_after` at an explicit date-time.
    #[must_use]
    pub fn is_stale_at(&self, now: crate::OkfDateTime) -> bool {
        self.inner.frontmatter.is_stale_at(now.into_core())
    }

    /// Returns the complete Attested Computation contract when applicable.
    #[must_use]
    pub fn attested_computation(&self) -> Option<OkfAttestedComputation> {
        self.inner.attested_computation().map(convert_computation)
    }
}

/// Builder for one OKF concept document.
#[derive(Debug, Clone, Default)]
pub struct OkfDocumentBuilder {
    frontmatter: Option<OkfFrontmatter>,
    body: String,
    inline_computation: Option<OkfInlineComputation>,
}

impl OkfDocumentBuilder {
    /// Sets complete frontmatter.
    #[must_use]
    pub fn frontmatter(mut self, frontmatter: OkfFrontmatter) -> Self {
        self.frontmatter = Some(frontmatter);
        self
    }

    /// Sets the Markdown body.
    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Adds the canonical inline section for an Attested Computation.
    #[must_use]
    pub fn inline_computation(mut self, computation: OkfInlineComputation) -> Self {
        self.inline_computation = Some(computation);
        self
    }

    /// Builds and validates a concept document.
    pub fn build(self) -> KnowledgeFormatResult<OkfDocument> {
        let frontmatter = self.frontmatter.ok_or_else(|| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                "OKF document frontmatter is required",
            )
        })?;
        let body = match self.inline_computation {
            Some(computation) => {
                if frontmatter.inner.get("computation").is_some()
                    || self.body.lines().any(|line| line.trim() == "# Computation")
                {
                    return Err(KnowledgeFormatError::new(
                        KnowledgeFormatErrorKind::InvalidOkf,
                        "inline computation conflicts with an existing computation source",
                    ));
                }
                let inline = computation.render()?;
                if self.body.trim().is_empty() {
                    inline
                } else {
                    format!("{}\n\n{inline}", self.body.trim_end())
                }
            }
            None => self.body,
        };
        let document = OkfDocument {
            inner: okf_core::Document::new(frontmatter.inner, body),
        };
        let report = document.validate();
        if !report.is_conformant() {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                "OKF document is not conformant",
            ));
        }
        Ok(document)
    }
}

fn convert_computation(value: okf_core::AttestedComputation) -> OkfAttestedComputation {
    let computation = match value.computation {
        okf_core::ComputationSource::Inline(inline) => OkfComputationSource::Inline {
            code: inline.code,
            language: inline.language,
            fenced: inline.fenced,
        },
        okf_core::ComputationSource::File(path) => OkfComputationSource::File(path),
        okf_core::ComputationSource::Missing => OkfComputationSource::Missing,
    };
    OkfAttestedComputation {
        runtime: value.runtime,
        parameters: value
            .parameters
            .into_iter()
            .map(|parameter| OkfParameter {
                name: parameter.name,
                type_name: parameter.type_,
                required: parameter.required,
            })
            .collect(),
        computation,
        executor: value.executor.map(|executor| OkfExecutor {
            resource: executor.resource,
            receipt: executor.receipt,
        }),
        attester: value.attester.map(|attester| OkfAttester {
            resource: attester.resource,
        }),
        has_redundant_inline: value.has_redundant_inline,
    }
}
