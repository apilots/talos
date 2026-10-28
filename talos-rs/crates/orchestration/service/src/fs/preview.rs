//! Worker filesystem ownership for ephemeral preview directory handles.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    FilePreviewBytes, FilePreviewMetadata, WorkerPreviewRequest, WorkerPreviewResponse,
};

#[derive(Default)]
pub(crate) struct PreviewRoots(Arc<Mutex<HashMap<String, PreviewRoot>>>);
struct PreviewRoot {
    owner: String,
    expires_at: i64,
    directory: Arc<talos_fs::RootedDirectory>,
}

impl PreviewRoots {
    pub(crate) async fn operate(
        &self,
        owner: String,
        request: WorkerPreviewRequest,
    ) -> TalosResult<WorkerPreviewResponse> {
        let now = chrono::Utc::now().timestamp();
        match request {
            WorkerPreviewRequest::Open {
                root,
                path,
                expires_at,
            } => {
                let (directory, resource) = tokio::task::spawn_blocking(move || {
                    let directory = talos_fs::RootedDirectory::open(&root)?;
                    let resource = directory.read(&path, 0..0)?;
                    Ok::<_, TalosError>((Arc::new(directory), resource))
                })
                .await
                .map_err(join_error)??;
                let resource_path = resource.path.to_string_lossy().replace('\\', "/");
                let metadata = content(resource, 0).metadata;
                let handle = uuid::Uuid::new_v4().to_string();
                let mut roots = self.0.lock().map_err(|_| state_error())?;
                roots.retain(|_, root| root.expires_at > now);
                roots.insert(
                    handle.clone(),
                    PreviewRoot {
                        owner,
                        expires_at,
                        directory,
                    },
                );
                let weak = Arc::downgrade(&self.0);
                let expired_handle = handle.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(
                        expires_at
                            .saturating_sub(chrono::Utc::now().timestamp())
                            .max(0) as u64,
                    ))
                    .await;
                    if let Some(roots) = weak.upgrade() {
                        if let Ok(mut roots) = roots.lock() {
                            roots.remove(&expired_handle);
                        }
                    }
                });
                Ok(WorkerPreviewResponse::Opened {
                    handle,
                    resource_path,
                    metadata,
                })
            }
            WorkerPreviewRequest::Read {
                handle,
                path,
                offset,
                limit,
            } => {
                if limit > talos_types::FILE_PREVIEW_MAX_READ_BYTES {
                    return Err(TalosError::new(
                        ErrorCode::InvalidRequest,
                        "Preview reads are limited to one MiB per request.",
                    ));
                }
                let directory = {
                    let mut roots = self.0.lock().map_err(|_| state_error())?;
                    roots.retain(|_, root| root.expires_at > now);
                    roots
                        .get(&handle)
                        .filter(|root| root.owner == owner)
                        .map(|root| root.directory.clone())
                        .ok_or_else(not_found)?
                };
                let resource = tokio::task::spawn_blocking(move || {
                    directory.read(&path, offset..offset.saturating_add(u64::from(limit)))
                })
                .await
                .map_err(join_error)??;
                Ok(WorkerPreviewResponse::Content(content(resource, offset)))
            }
            WorkerPreviewRequest::Close { handle } => {
                let mut roots = self.0.lock().map_err(|_| state_error())?;
                if roots.get(&handle).is_some_and(|root| root.owner != owner) {
                    return Err(not_found());
                }
                roots.remove(&handle);
                Ok(WorkerPreviewResponse::Closed)
            }
        }
    }
}
fn content(resource: talos_fs::RootedFileRead, offset: u64) -> FilePreviewBytes {
    let mime_type = crate::file::preview::mime_type(&resource.path, &resource.prefix);
    FilePreviewBytes {
        metadata: FilePreviewMetadata {
            name: resource
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            mime_type,
            size_bytes: resource.size_bytes,
        },
        offset,
        data: resource.bytes.into(),
    }
}
fn not_found() -> TalosError {
    TalosError::new(
        ErrorCode::ResourceNotFound,
        "The preview is unavailable or expired. Open the file again.",
    )
}
fn state_error() -> TalosError {
    TalosError::internal("Worker preview state is unavailable.")
}
fn join_error(error: tokio::task::JoinError) -> TalosError {
    TalosError::internal_with_source(
        "Read the preview resource; technical details were withheld.",
        error,
    )
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
