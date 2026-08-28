//! Pure in-memory OKF and managed knowledge document APIs.

mod error;
mod managed_document;
mod okf;

pub use error::{KnowledgeFormatError, KnowledgeFormatErrorKind, KnowledgeFormatResult};
pub use managed_document::*;
pub use okf::*;

#[cfg(test)]
#[path = "knowledge_format_tests.rs"]
mod tests;
