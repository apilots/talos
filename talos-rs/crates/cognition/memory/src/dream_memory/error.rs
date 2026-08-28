use talos_knowledge_format::{KnowledgeFormatError, KnowledgeFormatErrorKind};
use thiserror::Error;

/// Stable error categories for pure Dream Memory document operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DreamMemoryErrorKind {
    InvalidInput,
    InvalidDocument,
    Conflict,
    AlreadyExists,
    NotFound,
    LimitExceeded,
}

/// Error returned by the in-memory Dream Memory domain boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct DreamMemoryError {
    kind: DreamMemoryErrorKind,
    message: String,
}

impl DreamMemoryError {
    pub(crate) fn new(kind: DreamMemoryErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Returns the stable domain error category.
    #[must_use]
    pub const fn kind(&self) -> DreamMemoryErrorKind {
        self.kind
    }
}

impl From<KnowledgeFormatError> for DreamMemoryError {
    fn from(error: KnowledgeFormatError) -> Self {
        let kind = match error.kind() {
            KnowledgeFormatErrorKind::AlreadyExists => DreamMemoryErrorKind::AlreadyExists,
            KnowledgeFormatErrorKind::NotFound => DreamMemoryErrorKind::NotFound,
            KnowledgeFormatErrorKind::LimitExceeded => DreamMemoryErrorKind::LimitExceeded,
            KnowledgeFormatErrorKind::InvalidOkf
            | KnowledgeFormatErrorKind::InvalidProfile
            | KnowledgeFormatErrorKind::InvalidDocument
            | KnowledgeFormatErrorKind::InvalidEntry => DreamMemoryErrorKind::InvalidDocument,
        };
        Self::new(kind, error.to_string())
    }
}

/// Result type for pure Dream Memory document operations.
pub type DreamMemoryResult<T> = Result<T, DreamMemoryError>;
