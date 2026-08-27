use crate::{KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult};

/// Typed inline source for an Attested Computation producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfInlineComputation {
    code: String,
    language: Option<String>,
}

impl OkfInlineComputation {
    /// Creates an inline computation with no language hint.
    #[must_use]
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            language: None,
        }
    }

    /// Sets the fenced-code language hint.
    #[must_use]
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub(crate) fn render(&self) -> KnowledgeFormatResult<String> {
        if self.code.trim().is_empty()
            || self
                .language
                .as_deref()
                .is_some_and(|language| language.contains(['\n', '\r', '`']))
        {
            return Err(KnowledgeFormatError::new(
                KnowledgeFormatErrorKind::InvalidOkf,
                "inline computation must contain code and a valid language hint",
            ));
        }
        let fence = "`".repeat(max_backtick_run(&self.code).saturating_add(1).max(3));
        let language = self.language.as_deref().unwrap_or_default();
        Ok(format!(
            "# Computation\n\n{fence}{language}\n{}\n{fence}\n",
            self.code.trim_end()
        ))
    }
}

fn max_backtick_run(value: &str) -> usize {
    value
        .chars()
        .fold((0, 0), |(maximum, current), character| {
            let current = if character == '`' { current + 1 } else { 0 };
            (maximum.max(current), current)
        })
        .0
}

/// One typed Attested Computation parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfParameter {
    pub name: Option<String>,
    pub type_name: Option<String>,
    pub required: Option<bool>,
}

impl OkfParameter {
    /// Creates one declared computation parameter.
    #[must_use]
    pub fn new(name: impl Into<String>, type_name: impl Into<String>, required: bool) -> Self {
        Self {
            name: Some(name.into()),
            type_name: Some(type_name.into()),
            required: Some(required),
        }
    }
}

/// Attested Computation executor contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfExecutor {
    pub resource: Option<String>,
    pub receipt: Vec<String>,
}

impl OkfExecutor {
    /// Creates an executor contract.
    #[must_use]
    pub fn new(
        resource: impl Into<String>,
        receipt: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            resource: Some(resource.into()),
            receipt: receipt.into_iter().map(Into::into).collect(),
        }
    }
}

/// Attested Computation deterministic attester contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfAttester {
    pub resource: Option<String>,
}

impl OkfAttester {
    /// Creates a deterministic attester contract.
    #[must_use]
    pub fn new(resource: impl Into<String>) -> Self {
        Self {
            resource: Some(resource.into()),
        }
    }
}

/// Location of a sanctioned computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OkfComputationSource {
    Inline {
        code: String,
        language: Option<String>,
        fenced: bool,
    },
    File(String),
    Missing,
}

/// Complete Attested Computation format contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OkfAttestedComputation {
    pub runtime: Option<String>,
    pub parameters: Vec<OkfParameter>,
    pub computation: OkfComputationSource,
    pub executor: Option<OkfExecutor>,
    pub attester: Option<OkfAttester>,
    pub has_redundant_inline: bool,
}
