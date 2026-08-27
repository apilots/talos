use thiserror::Error;

/// Stable error categories exposed by the knowledge format boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeFormatErrorKind {
    InvalidOkf,
    InvalidProfile,
    InvalidDocument,
    InvalidEntry,
    AlreadyExists,
    NotFound,
    LimitExceeded,
}

/// Error returned by pure OKF and managed document operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct KnowledgeFormatError {
    kind: KnowledgeFormatErrorKind,
    message: String,
}

impl KnowledgeFormatError {
    /// Creates an error at an integration boundary that adds domain validation.
    pub fn new(kind: KnowledgeFormatErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Returns the stable error category.
    #[must_use]
    pub const fn kind(&self) -> KnowledgeFormatErrorKind {
        self.kind
    }
}

/// Result type for pure knowledge format operations.
pub type KnowledgeFormatResult<T> = Result<T, KnowledgeFormatError>;
