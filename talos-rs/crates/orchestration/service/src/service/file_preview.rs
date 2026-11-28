//! Unified ephemeral preview ownership. Transports never select a filesystem root.
use super::TalosService;
use crate::Actor;
use agent_protocol::{WorkerEnvironment, WorkerEnvironmentContext};
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

const PREVIEW_LIFETIME_SECONDS: i64 = 15 * 60;

pub(super) struct PreviewGrant {
    actor: Actor,
    descriptor: FilePreviewDescriptor,
    source: PreviewBinding,
    worker: Option<PreviewWorker>,
}

enum PreviewBinding {
    File {
        file_id: String,
    },
    Session {
        session_id: String,
        worktree_binding: Option<SessionWorktreeBinding>,
    },
}

struct PreviewWorker {
    environment: Arc<dyn WorkerEnvironment>,
    context: WorkerEnvironmentContext,
    handle: String,
}

impl TalosService {
    /// The product composition reuses the isolated preview origin for personal relay URLs.
    pub fn file_preview_origin(&self) -> Option<&str> {
        self.file_preview_base_url.as_deref()
    }

    pub async fn prepare_file_preview(
        &self,
        actor: &Actor,
        request: FilePreviewPrepareRequest,
    ) -> TalosResult<FilePreviewDescriptor> {
        let mut _admission = None;
        let (source, entry_path, metadata, worker) = match request.source {
            FilePreviewSource::File { file_id, node_id } => {
                let file = self
                    .files
                    .read(
                        actor,
                        FileReadRequest {
                            file_id: file_id.clone(),
                        },
                    )
                    .await?
                    .file;
                let metadata = FilePreviewMetadata {
                    name: file.original_filename,
                    mime_type: file.mime_type,
                    size_bytes: file.size_bytes,
                };
                let conversion = if crate::file::preview::needs_document(&metadata)
                    && metadata.size_bytes <= self.files.limits().max_upload_bytes
                {
                    let (admission, environment) = self
                        .active_worker_environment(actor, node_id.as_deref())
                        .await?;
                    _admission = admission;
                    let context = crate::access::worker_environment_context(actor);
                    let result = environment
                        .preview_resource(
                            context.clone(),
                            WorkerPreviewRequest::OpenFile {
                                metadata: metadata.clone(),
                                expires_at: chrono::Utc::now().timestamp()
                                    + PREVIEW_LIFETIME_SECONDS,
                                max_bytes: self.files.limits().max_upload_bytes,
                            },
                        )
                        .await?;
                    let WorkerPreviewResponse::Opened { handle, .. } = result else {
                        return Err(unexpected_result());
                    };
                    let worker = PreviewWorker {
                        environment,
                        context,
                        handle,
                    };
                    if let Err(error) = self
                        .write_preview_file(actor, &file_id, &metadata, &worker)
                        .await
                    {
                        if let Err(cleanup) = close_worker_preview(&worker).await {
                            talos_logging::report_error(&cleanup);
                        }
                        return Err(error);
                    }
                    Some(worker)
                } else {
                    None
                };
                (
                    PreviewBinding::File { file_id },
                    metadata.name.clone(),
                    metadata,
                    conversion,
                )
            }
            FilePreviewSource::SessionPath { session_id, path } => {
                let session = self.access.load_session(actor, &session_id).await?;
                let (admission, environment) = self
                    .active_worker_environment(actor, Some(&session.execution_node_id))
                    .await?;
                _admission = admission;
                let saved = self.sessions.build_agent_session_context(&session).await?;
                let context = WorkerEnvironmentContext {
                    user_id: saved.workspace.user_id,
                    username: actor.username.clone(),
                    workspace_id: Some(saved.workspace.workspace_id),
                };
                let root = self
                    .sessions
                    .resolve_working_directory(&session)
                    .await?
                    .unwrap_or(saved.workspace.root);
                let resource = environment
                    .preview_resource(
                        context.clone(),
                        WorkerPreviewRequest::Open {
                            root,
                            path: path.into(),
                            expires_at: chrono::Utc::now().timestamp() + PREVIEW_LIFETIME_SECONDS,
                        },
                    )
                    .await?;
                let WorkerPreviewResponse::Opened {
                    handle,
                    resource_path,
                    metadata,
                } = resource
                else {
                    return Err(unexpected_result());
                };
                (
                    PreviewBinding::Session {
                        session_id,
                        worktree_binding: session.worktree_binding,
                    },
                    resource_path,
                    metadata,
                    Some(PreviewWorker {
                        environment,
                        context,
                        handle,
                    }),
                )
            }
        };
        let preview_id = uuid::Uuid::new_v4().to_string();
        let resource_base_url = self
            .file_preview_base_url
            .as_ref()
            .map(|base| {
                let mut origin = url::Url::parse(base).map_err(|error| {
                    TalosError::internal_with_source(
                        "Resolve the configured preview origin.",
                        error,
                    )
                })?;
                let host = origin
                    .host_str()
                    .ok_or_else(|| TalosError::internal("Preview origin has no host."))?;
                origin
                    .set_host(Some(&format!("{preview_id}.{host}")))
                    .map_err(|error| {
                        TalosError::internal_with_source(
                            "Resolve the preview resource host.",
                            error,
                        )
                    })?;
                Ok::<_, TalosError>(origin.to_string())
            })
            .transpose()?;
        let mut descriptor = FilePreviewDescriptor {
            preview_id,
            resource_base_url,
            entry_path,
            metadata,
            document: None,
            expires_at: chrono::Utc::now().timestamp() + PREVIEW_LIFETIME_SECONDS,
        };
        let mut grant = PreviewGrant {
            actor: actor.clone(),
            descriptor: descriptor.clone(),
            source,
            worker,
        };
        if crate::file::preview::needs_document(&descriptor.metadata) {
            let limit = self.files.limits().max_upload_bytes;
            let document = if descriptor.metadata.size_bytes > limit {
                FilePreviewDocument::Unavailable {
                    reason: FilePreviewDocumentUnavailable::TooLarge,
                }
            } else {
                let worker = grant.worker.as_ref().ok_or_else(unexpected_result)?;
                match worker
                    .environment
                    .preview_resource(
                        worker.context.clone(),
                        WorkerPreviewRequest::PrepareDocument {
                            handle: worker.handle.clone(),
                            max_bytes: limit,
                        },
                    )
                    .await
                {
                    Ok(WorkerPreviewResponse::Document(document)) => document,
                    result => {
                        if let Err(cleanup) = close_worker_preview(worker).await {
                            talos_logging::report_error(&cleanup);
                        }
                        return Err(result.err().unwrap_or_else(unexpected_result));
                    }
                }
            };
            descriptor.document = Some(document);
            grant.descriptor = descriptor.clone();
        }
        let mut previews = self
            .file_previews
            .lock()
            .map_err(|_| preview_state_error())?;
        previews.insert(descriptor.preview_id.clone(), Arc::new(grant));
        let weak = Arc::downgrade(&self.file_previews);
        let expired_id = descriptor.preview_id.clone();
        let lifetime = descriptor
            .expires_at
            .saturating_sub(chrono::Utc::now().timestamp())
            .max(0) as u64;
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(lifetime)).await;
            if let Some(previews) = weak.upgrade() {
                if let Ok(mut previews) = previews.lock() {
                    previews.remove(&expired_id);
                }
            }
        });
        Ok(descriptor)
    }

    pub async fn read_file_preview(
        &self,
        actor: &Actor,
        request: FilePreviewReadRequest,
    ) -> TalosResult<FilePreviewBytes> {
        let grant = self.preview_grant(&request.preview_id)?;
        if grant.actor.user_id != actor.user_id {
            return Err(preview_not_found());
        }
        self.read_preview_grant(&grant, actor, request).await
    }

    /// Only resource-origin transports use the scoped opaque capability directly.
    /// SDK/RPC callers always use actor-authorized read_file_preview instead.
    pub async fn read_file_preview_capability(
        &self,
        request: FilePreviewReadRequest,
    ) -> TalosResult<FilePreviewBytes> {
        let grant = self.preview_grant(&request.preview_id)?;
        self.read_preview_grant(&grant, &grant.actor, request).await
    }

    async fn read_preview_grant(
        &self,
        grant: &PreviewGrant,
        actor: &Actor,
        request: FilePreviewReadRequest,
    ) -> TalosResult<FilePreviewBytes> {
        if request.limit > talos_types::FILE_PREVIEW_MAX_READ_BYTES {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Preview reads are limited to one MiB per request.",
            ));
        }
        if let Some(FilePreviewDocument::Pdf { resource_path }) = &grant.descriptor.document {
            if resource_path == &request.resource_path {
                match &grant.source {
                    PreviewBinding::File { file_id } => {
                        self.files
                            .read(
                                actor,
                                FileReadRequest {
                                    file_id: file_id.clone(),
                                },
                            )
                            .await?;
                    }
                    PreviewBinding::Session {
                        session_id,
                        worktree_binding,
                        ..
                    } => {
                        self.check_session_preview(actor, session_id, worktree_binding.as_ref())
                            .await?;
                    }
                }
                let worker = grant.worker.as_ref().ok_or_else(unexpected_result)?;
                return read_worker_preview(worker, request).await;
            }
        }
        self.read_preview_source(grant, actor, request).await
    }

    async fn write_preview_file(
        &self,
        actor: &Actor,
        file_id: &str,
        metadata: &FilePreviewMetadata,
        worker: &PreviewWorker,
    ) -> TalosResult<()> {
        let mut offset = 0;
        while offset < metadata.size_bytes {
            let end = offset
                .saturating_add(u64::from(FILE_PREVIEW_MAX_READ_BYTES))
                .min(metadata.size_bytes);
            let part = self
                .files
                .read_preview_range(actor, file_id, offset..end)
                .await?;
            if part.file.size_bytes != metadata.size_bytes
                || part.bytes.len() as u64 != end - offset
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "The file changed while preparing its preview.",
                ));
            }
            let result = worker
                .environment
                .preview_resource(
                    worker.context.clone(),
                    WorkerPreviewRequest::Write {
                        handle: worker.handle.clone(),
                        offset,
                        data: part.bytes,
                    },
                )
                .await?;
            if !matches!(result, WorkerPreviewResponse::Written) {
                return Err(unexpected_result());
            }
            offset = end;
        }
        Ok(())
    }

    async fn check_session_preview(
        &self,
        actor: &Actor,
        session_id: &str,
        authorized: Option<&SessionWorktreeBinding>,
    ) -> TalosResult<()> {
        let session = self.access.load_session(actor, session_id).await?;
        if authorized != session.worktree_binding.as_ref() {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "The Session working directory changed. Open the file again.",
            ));
        }
        Ok(())
    }

    async fn read_preview_source(
        &self,
        grant: &PreviewGrant,
        actor: &Actor,
        request: FilePreviewReadRequest,
    ) -> TalosResult<FilePreviewBytes> {
        match &grant.source {
            PreviewBinding::File { file_id } => {
                if request.resource_path != grant.descriptor.entry_path {
                    return Err(preview_not_found());
                }
                let range = request.offset..request.offset.saturating_add(u64::from(request.limit));
                let content = self.files.read_preview_range(actor, file_id, range).await?;
                Ok(FilePreviewBytes {
                    metadata: FilePreviewMetadata {
                        name: content.file.original_filename,
                        mime_type: content.file.mime_type,
                        size_bytes: content.file.size_bytes,
                    },
                    offset: request.offset,
                    data: content.bytes,
                })
            }
            PreviewBinding::Session {
                session_id,
                worktree_binding,
            } => {
                self.check_session_preview(actor, session_id, worktree_binding.as_ref())
                    .await?;
                let worker = grant.worker.as_ref().ok_or_else(unexpected_result)?;
                read_worker_preview(worker, request).await
            }
        }
    }

    pub async fn close_file_preview(
        &self,
        actor: &Actor,
        request: FilePreviewCloseRequest,
    ) -> TalosResult<FilePreviewCloseResponse> {
        let removed = {
            let mut previews = self
                .file_previews
                .lock()
                .map_err(|_| preview_state_error())?;
            if previews
                .get(&request.preview_id)
                .is_some_and(|grant| grant.actor.user_id != actor.user_id)
            {
                return Err(preview_not_found());
            }
            previews.remove(&request.preview_id)
        };
        if let Some(worker) = removed.as_ref().and_then(|grant| grant.worker.as_ref()) {
            close_worker_preview(worker).await?;
        }
        Ok(FilePreviewCloseResponse {
            closed: removed.is_some(),
        })
    }

    fn preview_grant(&self, preview_id: &str) -> TalosResult<Arc<PreviewGrant>> {
        let mut previews = self
            .file_previews
            .lock()
            .map_err(|_| preview_state_error())?;
        let grant = previews
            .get(preview_id)
            .cloned()
            .ok_or_else(preview_not_found)?;
        if grant.descriptor.expires_at <= chrono::Utc::now().timestamp() {
            previews.remove(preview_id);
            return Err(preview_not_found());
        }
        Ok(grant)
    }
}
fn preview_not_found() -> TalosError {
    TalosError::new(
        ErrorCode::ResourceNotFound,
        "The preview is unavailable or expired. Open the file again.",
    )
}
fn preview_state_error() -> TalosError {
    TalosError::internal("File preview state is unavailable.")
}

fn unexpected_result() -> TalosError {
    TalosError::internal("Worker returned an unexpected preview result.")
}
async fn close_worker_preview(worker: &PreviewWorker) -> TalosResult<()> {
    match worker
        .environment
        .preview_resource(
            worker.context.clone(),
            WorkerPreviewRequest::Close {
                handle: worker.handle.clone(),
            },
        )
        .await?
    {
        WorkerPreviewResponse::Closed => Ok(()),
        _ => Err(unexpected_result()),
    }
}
async fn read_worker_preview(
    worker: &PreviewWorker,
    request: FilePreviewReadRequest,
) -> TalosResult<FilePreviewBytes> {
    match worker
        .environment
        .preview_resource(
            worker.context.clone(),
            WorkerPreviewRequest::Read {
                handle: worker.handle.clone(),
                path: request.resource_path.into(),
                offset: request.offset,
                limit: request.limit,
            },
        )
        .await?
    {
        WorkerPreviewResponse::Content(content) => Ok(content),
        _ => Err(unexpected_result()),
    }
}
