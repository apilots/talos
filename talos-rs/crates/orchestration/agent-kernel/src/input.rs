use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosFileErrorKind, TalosResult};

/// Normalized input for one Agent turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserInput {
    /// Text projection consumed by prompt, title, memory, and compaction paths.
    pub text: String,
    /// Ordered file references resolved only for the current model turn.
    #[serde(default)]
    pub file_ids: Vec<String>,
}

/// Authorization scope used while resolving one turn's file references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFileScope {
    /// Authenticated owner of every referenced file.
    pub owner_user_id: String,
    /// Project bound to the current session, when present.
    pub project_id: Option<String>,
}

/// Ordered, authorized file content resolved for one model turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAttachment {
    pub file_id: String,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub bytes: Bytes,
}

/// Resolves ordered, authorized file references for one model turn.
///
/// Implementations must authorize the complete input as one operation and
/// return results in `file_ids` order without exposing storage coordinates.
pub trait AttachmentReader: Send + Sync {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>>;
}

/// Explicit reader for text-only Agent deployments and tests.
pub struct RejectingAttachmentReader;

impl AttachmentReader for RejectingAttachmentReader {
    fn read<'a>(
        &'a self,
        _scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>> {
        Box::pin(async move {
            if file_ids.is_empty() {
                Ok(Vec::new())
            } else {
                Err(TalosError::FileError {
                    kind: TalosFileErrorKind::UnsupportedAttachment,
                    message: "file attachment reader is not configured".to_string(),
                })
            }
        })
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
