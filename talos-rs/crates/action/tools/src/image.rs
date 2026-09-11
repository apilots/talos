use futures_util::future::BoxFuture;
use talos_error::TalosResult;
use talos_llm_provider::Attachment;
use talos_types::FileInfo;

/// Application-owned storage for images produced by tools.
///
/// The caller supplies the authenticated user from ToolContext, never model arguments.
/// Implementations reuse file authorization and byte/MIME limits. Each image uses
/// the file lifecycle; results preserve input order. Reads are restricted to the
/// user's personal files. Completed files remain manageable after a later write
/// failure or cancellation. Bytes must not enter events or persisted messages.
pub trait ToolImageStore: Send + Sync {
    fn save<'a>(
        &'a self,
        user_id: &'a str,
        images: Vec<Attachment>,
    ) -> BoxFuture<'a, TalosResult<Vec<FileInfo>>>;
    fn read<'a>(
        &'a self,
        user_id: &'a str,
        file_ids: &'a [String],
    ) -> BoxFuture<'a, TalosResult<Vec<Attachment>>>;
}
