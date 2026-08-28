mod bundle;
mod document;
mod entry_codec;
mod error;
mod profile;
mod types;

pub use bundle::*;
pub use document::DreamMemoryDocument;
pub use error::{DreamMemoryError, DreamMemoryErrorKind, DreamMemoryResult};
pub use types::{
    DreamMemoryAppendResult, DreamMemoryDate, DreamMemoryDocumentContext, DreamMemoryEntry,
    DreamMemoryEntryBuilder, DreamMemoryProvenance, DreamMemorySourceRange, MemoryAgentId,
    MemoryId, MemorySessionId, MemoryTimezone, MemoryUserId,
};

#[cfg(test)]
#[path = "dream_memory_tests.rs"]
mod tests;
