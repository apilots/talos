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
type PreviewDocument = Arc<Mutex<Option<crate::file::preview::PreparedDocument>>>;
struct PreviewRoot {
    owner: String,
    expires_at: i64,
    source: PreviewSource,
    document: PreviewDocument,
}
#[derive(Clone)]
enum PreviewSource {
    Directory {
        directory: Arc<talos_fs::RootedDirectory>,
        path: std::path::PathBuf,
    },
    File {
        metadata: FilePreviewMetadata,
        bytes: Arc<Mutex<Vec<u8>>>,
    },
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
                let entry_path = resource.path.clone();
                let resource_path = resource.path.to_string_lossy().replace('\\', "/");
                let metadata = content(resource, 0).metadata;
                let handle = self.insert(
                    owner,
                    expires_at,
                    PreviewSource::Directory {
                        directory,
                        path: entry_path,
                    },
                )?;
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
                let (source, document) = self.resource(&owner, &handle, now)?;
                if let Some(document) = document
                    .lock()
                    .map_err(|_| state_error())?
                    .as_ref()
                    .filter(|document| document.handles(&path.to_string_lossy()))
                {
                    return Ok(WorkerPreviewResponse::Content(document.read(offset, limit)));
                }
                let resource = match source {
                    PreviewSource::Directory { directory, .. } => {
                        let resource = tokio::task::spawn_blocking(move || {
                            directory.read(&path, offset..offset.saturating_add(u64::from(limit)))
                        })
                        .await
                        .map_err(join_error)??;
                        content(resource, offset)
                    }
                    PreviewSource::File { .. } => return Err(not_found()),
                };
                Ok(WorkerPreviewResponse::Content(resource))
            }
            WorkerPreviewRequest::OpenFile {
                metadata,
                expires_at,
                max_bytes,
            } => {
                if metadata.size_bytes > max_bytes {
                    return Err(input_limit());
                }
                let resource_path = metadata.name.clone();
                let handle = self.insert(
                    owner,
                    expires_at,
                    PreviewSource::File {
                        metadata: metadata.clone(),
                        bytes: Arc::default(),
                    },
                )?;
                Ok(WorkerPreviewResponse::Opened {
                    handle,
                    resource_path,
                    metadata,
                })
            }
            WorkerPreviewRequest::Write {
                handle,
                offset,
                data,
            } => {
                let (source, _) = self.resource(&owner, &handle, now)?;
                let PreviewSource::File { metadata, bytes } = source else {
                    return Err(invalid_input());
                };
                let mut bytes = bytes.lock().map_err(|_| state_error())?;
                if data.len() > talos_types::FILE_PREVIEW_MAX_READ_BYTES as usize
                    || offset != bytes.len() as u64
                    || offset.saturating_add(data.len() as u64) > metadata.size_bytes
                {
                    return Err(invalid_input());
                }
                bytes.extend_from_slice(&data);
                Ok(WorkerPreviewResponse::Written)
            }
            WorkerPreviewRequest::PrepareDocument { handle, max_bytes } => {
                let (source, document) = self.resource(&owner, &handle, now)?;
                let (metadata, bytes) = match source {
                    PreviewSource::Directory { directory, path } => {
                        tokio::task::spawn_blocking(move || {
                            let metadata = content(directory.read(&path, 0..0)?, 0).metadata;
                            if metadata.size_bytes > max_bytes {
                                return Ok((metadata, None));
                            }
                            let bytes = directory.read(&path, 0..metadata.size_bytes)?;
                            if bytes.size_bytes != metadata.size_bytes
                                || bytes.bytes.len() as u64 != metadata.size_bytes
                            {
                                return Err(invalid_input());
                            }
                            Ok::<_, TalosError>((metadata, Some(bytes.bytes.into())))
                        })
                        .await
                        .map_err(join_error)??
                    }
                    PreviewSource::File { metadata, bytes } => {
                        let mut bytes = bytes.lock().map_err(|_| state_error())?;
                        if bytes.len() as u64 != metadata.size_bytes {
                            return Err(invalid_input());
                        }
                        let data = (metadata.size_bytes <= max_bytes)
                            .then(|| std::mem::take(&mut *bytes).into());
                        (metadata, data)
                    }
                };
                let (description, prepared) = match bytes {
                    Some(bytes) => crate::file::preview::prepare(&metadata, bytes, max_bytes).await,
                    None => (
                        talos_types::FilePreviewDocument::Unavailable {
                            reason: talos_types::FilePreviewDocumentUnavailable::TooLarge,
                        },
                        None,
                    ),
                };
                *document.lock().map_err(|_| state_error())? = prepared;
                Ok(WorkerPreviewResponse::Document(description))
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
    fn insert(&self, owner: String, expires_at: i64, source: PreviewSource) -> TalosResult<String> {
        let handle = uuid::Uuid::new_v4().to_string();
        self.0.lock().map_err(|_| state_error())?.insert(
            handle.clone(),
            PreviewRoot {
                owner,
                expires_at,
                source,
                document: Arc::default(),
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
        Ok(handle)
    }
    fn resource(
        &self,
        owner: &str,
        handle: &str,
        now: i64,
    ) -> TalosResult<(PreviewSource, PreviewDocument)> {
        let roots = self.0.lock().map_err(|_| state_error())?;
        let root = roots
            .get(handle)
            .filter(|root| root.owner == owner && root.expires_at > now)
            .ok_or_else(not_found)?;
        Ok((root.source.clone(), root.document.clone()))
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

fn input_limit() -> TalosError {
    TalosError::new(
        ErrorCode::AttachmentLimitExceeded,
        "The document exceeds the preview byte limit.",
    )
}
fn invalid_input() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "The preview input does not match its authorized metadata.",
    )
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
