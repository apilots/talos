use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use talos_error::TalosResult;

/// Authorization scope used while resolving one Turn file input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFileScope {
    /// Authenticated owner of every referenced file.
    pub owner_user_id: String,
    /// Project bound to the current Session, when present.
    pub project_id: Option<String>,
}

/// Ordered, authorized file content resolved for one Agent Turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAttachment {
    pub file_id: String,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub bytes: Bytes,
}

/// Provider-neutral port for resolving structured file input.
///
/// Service implementations own authorization and storage I/O. Agent Provider
/// implementations consume only ordered content and must not depend on Service,
/// Store records, or storage coordinates.
pub trait AttachmentReader: Send + Sync {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>>;
}
