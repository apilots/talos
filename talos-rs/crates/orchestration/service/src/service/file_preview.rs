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
    document: Option<crate::file::preview::PreparedDocument>,
}

enum PreviewBinding {
    File {
        file_id: String,
    },
    Session {
        session_id: String,
        environment: Arc<dyn WorkerEnvironment>,
        context: WorkerEnvironmentContext,
        handle: String,
    },
}

impl TalosService {
    pub async fn prepare_file_preview(
        &self,
        actor: &Actor,
        request: FilePreviewPrepareRequest,
    ) -> TalosResult<FilePreviewDescriptor> {
        let (source, entry_path, metadata) = match request.source {
            FilePreviewSource::File { file_id } => {
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
                (
                    PreviewBinding::File { file_id },
                    metadata.name.clone(),
                    metadata,
                )
            }
            FilePreviewSource::SessionPath { session_id, path } => {
                let session = self.access.load_session(actor, &session_id).await?;
                let saved = self.sessions.build_agent_session_context(&session).await?;
                let context = WorkerEnvironmentContext {
                    user_id: saved.workspace.user_id,
                    username: actor.username.clone(),
                    workspace_id: Some(saved.workspace.workspace_id),
                };
                let root = session.current_directory.unwrap_or(saved.workspace.root);
                let environment = self
                    .worker_environment(actor, Some(&session.execution_node_id))
                    .await?;
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
                    return Err(TalosError::internal(
                        "Worker returned an unexpected preview result.",
                    ));
                };
                (
                    PreviewBinding::Session {
                        session_id,
                        environment,
                        context,
                        handle,
                    },
                    resource_path,
                    metadata,
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
            document: None,
        };
        if crate::file::preview::needs_document(&descriptor.metadata) {
            let limit = self.files.limits().max_upload_bytes;
            let document = if descriptor.metadata.size_bytes > limit {
                (
                    FilePreviewDocument::Unavailable {
                        reason: FilePreviewDocumentUnavailable::TooLarge,
                    },
                    None,
                )
            } else {
                let bytes = self.preview_source_bytes(&grant, actor, limit).await;
                match bytes {
                    Ok(bytes) => {
                        crate::file::preview::prepare(&descriptor.metadata, bytes, limit).await
                    }
                    Err(error) => {
                        if let PreviewBinding::Session {
                            environment,
                            context,
                            handle,
                            ..
                        } = &grant.source
                        {
                            if let Err(error) = environment
                                .preview_resource(
                                    context.clone(),
                                    WorkerPreviewRequest::Close {
                                        handle: handle.clone(),
                                    },
                                )
                                .await
                            {
                                talos_logging::report_error(&error);
                            }
                        }
                        return Err(error);
                    }
                }
            };
            descriptor.document = Some(document.0);
            grant.descriptor = descriptor.clone();
            grant.document = document.1;
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
        if let Some(document) = grant
            .document
            .as_ref()
            .filter(|document| document.handles(&request.resource_path))
        {
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
                PreviewBinding::Session { session_id, .. } => {
                    self.access.load_session(actor, session_id).await?;
                }
            }
            return Ok(document.read(request.offset, request.limit));
        }
        self.read_preview_source(grant, actor, request).await
    }

    async fn preview_source_bytes(
        &self,
        grant: &PreviewGrant,
        actor: &Actor,
        limit: u64,
    ) -> TalosResult<bytes::Bytes> {
        let mut request = FilePreviewReadRequest {
            preview_id: grant.descriptor.preview_id.clone(),
            resource_path: grant.descriptor.entry_path.clone(),
            offset: 0,
            limit: 0,
        };
        let metadata = self
            .read_preview_source(grant, actor, request.clone())
            .await?
            .metadata;
        if metadata.size_bytes > limit {
            return Err(TalosError::new(
                ErrorCode::AttachmentLimitExceeded,
                "The document exceeds the preview byte limit.",
            ));
        }
        let mut bytes = Vec::new();
        while request.offset < metadata.size_bytes {
            request.limit = (metadata.size_bytes - request.offset)
                .min(u64::from(FILE_PREVIEW_MAX_READ_BYTES)) as u32;
            let part = self
                .read_preview_source(grant, actor, request.clone())
                .await?;
            let chunk = part.data;
            if part.metadata.size_bytes != metadata.size_bytes
                || chunk.is_empty()
                || chunk.len() as u64 > metadata.size_bytes - request.offset
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "The file changed while preparing its preview.",
                ));
            }
            request.offset += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes.into())
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
                environment,
                context,
                handle,
            } => {
                self.access.load_session(actor, session_id).await?;
                let content = environment
                    .preview_resource(
                        context.clone(),
                        WorkerPreviewRequest::Read {
                            handle: handle.clone(),
                            path: request.resource_path.into(),
                            offset: request.offset,
                            limit: request.limit,
                        },
                    )
                    .await?;
                match content {
                    WorkerPreviewResponse::Content(content) => Ok(content),
                    _ => Err(TalosError::internal(
                        "Worker returned an unexpected preview result.",
                    )),
                }
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
        if let Some(grant) = &removed {
            if let PreviewBinding::Session {
                environment,
                context,
                handle,
                ..
            } = &grant.source
            {
                environment
                    .preview_resource(
                        context.clone(),
                        WorkerPreviewRequest::Close {
                            handle: handle.clone(),
                        },
                    )
                    .await?;
            }
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
