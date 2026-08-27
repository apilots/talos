use crate::{
    KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult, OkfAttester,
    OkfExecutor, OkfExtensionFields, OkfGenerated, OkfParameter, OkfSource, OkfStatus,
    OkfVerification,
};

/// Typed facade over complete, extension-preserving OKF frontmatter.
#[derive(Debug, Clone, PartialEq)]
pub struct OkfFrontmatter {
    pub(crate) inner: okf_core::Frontmatter,
}

impl Default for OkfFrontmatter {
    fn default() -> Self {
        Self {
            inner: okf_core::Frontmatter::new(),
        }
    }
}

impl OkfFrontmatter {
    /// Starts a frontmatter builder.
    #[must_use]
    pub fn builder() -> OkfFrontmatterBuilder {
        OkfFrontmatterBuilder::default()
    }

    /// Required concept type, when present as a scalar.
    #[must_use]
    pub fn concept_type(&self) -> Option<String> {
        self.inner.type_().map(|value| value.into_owned())
    }

    /// Optional display title.
    #[must_use]
    pub fn title(&self) -> Option<String> {
        self.inner.title().map(|value| value.into_owned())
    }

    /// Optional description.
    #[must_use]
    pub fn description(&self) -> Option<String> {
        self.inner.description().map(|value| value.into_owned())
    }

    /// Optional resource URI or path.
    #[must_use]
    pub fn resource(&self) -> Option<String> {
        self.inner.resource().map(|value| value.into_owned())
    }

    /// Producer tags.
    #[must_use]
    pub fn tags(&self) -> Vec<String> {
        self.inner.tags()
    }

    /// Lifecycle status with permissive unknown-value support.
    #[must_use]
    pub fn status(&self) -> OkfStatus {
        self.inner.status().into()
    }

    /// Shared provenance usage window.
    #[must_use]
    pub fn usage_window(&self) -> Option<crate::OkfUsageWindow> {
        self.inner
            .usage_window()
            .map(|window| crate::OkfUsageWindow {
                from: window
                    .from
                    .map(|value| crate::OkfDateTimeValue::from_raw(value.raw)),
                to: window
                    .to
                    .map(|value| crate::OkfDateTimeValue::from_raw(value.raw)),
            })
    }

    /// Lifecycle staleness threshold as written by the producer.
    #[must_use]
    pub fn stale_after(&self) -> Option<String> {
        self.inner.stale_after().map(|value| value.raw)
    }

    /// All producer extension fields.
    #[must_use]
    pub fn extensions(&self) -> OkfExtensionFields {
        OkfExtensionFields::from_frontmatter(&self.inner)
    }
}

/// Builder for extension-preserving OKF frontmatter.
#[derive(Debug, Clone, Default)]
pub struct OkfFrontmatterBuilder {
    frontmatter: OkfFrontmatter,
}

impl OkfFrontmatterBuilder {
    /// Sets the required concept type.
    #[must_use]
    pub fn concept_type(mut self, value: impl Into<String>) -> Self {
        self.set_string("type", value);
        self
    }

    /// Sets the optional title.
    #[must_use]
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.set_string("title", value);
        self
    }

    /// Sets the optional description.
    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.set_string("description", value);
        self
    }

    /// Sets the optional resource.
    #[must_use]
    pub fn resource(mut self, value: impl Into<String>) -> Self {
        self.set_string("resource", value);
        self
    }

    /// Sets tags.
    #[must_use]
    pub fn tags(mut self, values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.frontmatter.inner.set(
            "tags",
            okf_core::yaml::Value::Sequence(
                values
                    .into_iter()
                    .map(|value| okf_core::yaml::Value::String(value.into()))
                    .collect(),
            ),
        );
        self
    }

    /// Sets lifecycle status.
    #[must_use]
    pub fn status(mut self, status: OkfStatus) -> Self {
        self.set_string("status", status.as_str());
        self
    }

    /// Sets the producer identity and generation timestamp.
    #[must_use]
    pub fn generated(mut self, generated: OkfGenerated) -> Self {
        self.frontmatter
            .inner
            .set("generated", generated_value(generated));
        self
    }

    /// Sets all verification events using the canonical list shape.
    #[must_use]
    pub fn verifications(mut self, values: impl IntoIterator<Item = OkfVerification>) -> Self {
        self.frontmatter.inner.set(
            "verified",
            okf_core::yaml::Value::Sequence(values.into_iter().map(verification_value).collect()),
        );
        self
    }

    /// Sets typed provenance sources.
    #[must_use]
    pub fn sources(mut self, values: impl IntoIterator<Item = OkfSource>) -> Self {
        self.frontmatter.inner.set(
            "sources",
            okf_core::yaml::Value::Sequence(values.into_iter().map(source_value).collect()),
        );
        self
    }

    /// Sets the shared provenance usage window.
    #[must_use]
    pub fn usage_window(mut self, value: crate::OkfUsageWindow) -> Self {
        self.frontmatter
            .inner
            .set("usage_window", usage_window_value(value));
        self
    }

    /// Sets the lifecycle staleness threshold.
    #[must_use]
    pub fn stale_after(mut self, value: crate::OkfDateTime) -> Self {
        self.set_string("stale_after", value.to_iso8601());
        self
    }

    /// Sets the legacy v0.1 timestamp for producer compatibility.
    #[must_use]
    pub fn legacy_timestamp(mut self, value: crate::OkfDateTime) -> Self {
        self.set_string("timestamp", value.to_iso8601());
        self
    }

    /// Sets the runtime for an Attested Computation concept.
    #[must_use]
    pub fn computation_runtime(mut self, value: impl Into<String>) -> Self {
        self.set_string("runtime", value);
        self
    }

    /// Sets typed Attested Computation parameters.
    #[must_use]
    pub fn computation_parameters(
        mut self,
        values: impl IntoIterator<Item = OkfParameter>,
    ) -> Self {
        self.frontmatter.inner.set(
            "parameters",
            okf_core::yaml::Value::Sequence(values.into_iter().map(parameter_value).collect()),
        );
        self
    }

    /// Sets a file-backed computation path.
    #[must_use]
    pub fn computation_resource(mut self, value: impl Into<String>) -> Self {
        self.set_string("computation", value);
        self
    }

    /// Sets the typed executor contract.
    #[must_use]
    pub fn executor(mut self, value: OkfExecutor) -> Self {
        self.frontmatter
            .inner
            .set("executor", executor_value(value));
        self
    }

    /// Sets the typed deterministic attester contract.
    #[must_use]
    pub fn attester(mut self, value: OkfAttester) -> Self {
        self.frontmatter
            .inner
            .set("attester", attester_value(value));
        self
    }

    /// Validates the required OKF core and returns frontmatter.
    pub fn build(self) -> KnowledgeFormatResult<OkfFrontmatter> {
        let document = okf_core::Document::new(self.frontmatter.inner.clone(), "");
        document.validate().map_err(|error| {
            KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                format!("invalid OKF frontmatter: {error}"),
            )
        })?;
        Ok(self.frontmatter)
    }

    fn set_string(&mut self, key: &str, value: impl Into<String>) {
        self.frontmatter
            .inner
            .set(key, okf_core::yaml::Value::String(value.into()));
    }
}

fn generated_value(value: OkfGenerated) -> okf_core::yaml::Value {
    mapping_value([
        value.by.map(|actor| ("by", actor.as_str().to_string())),
        value.at.map(|at| ("at", at.as_str())),
    ])
}

fn verification_value(value: OkfVerification) -> okf_core::yaml::Value {
    mapping_value([
        value.by.map(|actor| ("by", actor.as_str().to_string())),
        value.at.map(|at| ("at", at.as_str())),
    ])
}

fn usage_window_value(value: crate::OkfUsageWindow) -> okf_core::yaml::Value {
    mapping_value([
        value.from.map(|from| ("from", from.as_str())),
        value.to.map(|to| ("to", to.as_str())),
    ])
}

fn source_value(value: OkfSource) -> okf_core::yaml::Value {
    let mut mapping = okf_core::yaml::Mapping::new();
    for (key, value) in [
        ("id", value.id),
        ("resource", value.resource),
        ("title", value.title),
        (
            "author",
            value.author.map(|actor| actor.as_str().to_string()),
        ),
        (
            "last_modified",
            value
                .last_modified
                .map(|last_modified| last_modified.as_str()),
        ),
    ] {
        if let Some(value) = value {
            mapping.insert(key, okf_core::yaml::Value::String(value));
        }
    }
    if let Some(usage_count) = value.usage_count {
        mapping.insert("usage_count", okf_core::yaml::Value::Int(usage_count));
    }
    if let Some(window) = value.usage_window {
        mapping.insert("usage_window", usage_window_value(window));
    }
    okf_core::yaml::Value::Mapping(mapping)
}

fn parameter_value(value: OkfParameter) -> okf_core::yaml::Value {
    let mut mapping = okf_core::yaml::Mapping::new();
    for (key, value) in [("name", value.name), ("type", value.type_name)] {
        if let Some(value) = value {
            mapping.insert(key, okf_core::yaml::Value::String(value));
        }
    }
    if let Some(required) = value.required {
        mapping.insert("required", okf_core::yaml::Value::Bool(required));
    }
    okf_core::yaml::Value::Mapping(mapping)
}

fn executor_value(value: OkfExecutor) -> okf_core::yaml::Value {
    let mut mapping = okf_core::yaml::Mapping::new();
    if let Some(resource) = value.resource {
        mapping.insert("resource", okf_core::yaml::Value::String(resource));
    }
    mapping.insert(
        "receipt",
        okf_core::yaml::Value::Sequence(
            value
                .receipt
                .into_iter()
                .map(okf_core::yaml::Value::String)
                .collect(),
        ),
    );
    okf_core::yaml::Value::Mapping(mapping)
}

fn attester_value(value: OkfAttester) -> okf_core::yaml::Value {
    mapping_value([value.resource.map(|resource| ("resource", resource))])
}

fn mapping_value<const N: usize>(
    fields: [Option<(&'static str, String)>; N],
) -> okf_core::yaml::Value {
    let mut mapping = okf_core::yaml::Mapping::new();
    for (key, value) in fields.into_iter().flatten() {
        mapping.insert(key, okf_core::yaml::Value::String(value));
    }
    okf_core::yaml::Value::Mapping(mapping)
}
