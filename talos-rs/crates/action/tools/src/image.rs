use futures_util::future::BoxFuture;
use talos_error::TalosResult;
use talos_llm_provider::Attachment;
use talos_types::FileInfo;

/// Application-owned File storage for tool images and completed output files.
///
/// The caller supplies the authenticated user from ToolContext, never model arguments.
/// Implementations reuse file authorization and byte/MIME limits. Each image uses
/// the file lifecycle; results preserve input order. Reads are restricted to the
/// user's personal files. Completed files remain manageable after a later write
/// failure or cancellation. Bytes must not enter events or persisted messages.
pub trait ToolImageStore: Send + Sync {
    /// Completed tool files reuse the authenticated File owner and its normal limits.
    fn save_files<'a>(
        &'a self,
        _user_id: &'a str,
        _files: Vec<talos_types::ToolOutputFile>,
    ) -> BoxFuture<'a, TalosResult<Vec<FileInfo>>> {
        Box::pin(async {
            Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Tool file storage is unavailable.",
            ))
        })
    }

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
