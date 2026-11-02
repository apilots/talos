use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use talos_agent::{AttachmentReader, ResolvedAttachment, TurnFileScope};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::file::{FileRecord, FileStatus};

use super::{
    service::{file_not_found, opendal_error},
    FileService,
};

impl AttachmentReader for FileService {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>> {
        Box::pin(self.read_attachments(scope, file_ids))
    }
}

impl FileService {
    /// Reads one ready actor-owned file using the same byte limits as Turn input.
    pub async fn read_content(
        &self,
        actor: &crate::Actor,
        request: talos_types::FileReadRequest,
    ) -> TalosResult<super::FileContent> {
        let file = self.read(actor, request).await?.file;
        let scope = TurnFileScope {
            owner_user_id: actor.user_id.clone(),
            project_id: match &file.scope {
                talos_types::FileScope::Personal => None,
                talos_types::FileScope::Project { project_id } => Some(project_id.clone()),
            },
        };
        let resolved = self
            .read_attachments(&scope, std::slice::from_ref(&file.file_id))
            .await?
            .pop()
            .ok_or_else(|| TalosError::internal("File content reader returned no result."))?;
        Ok(super::FileContent {
            file,
            bytes: resolved.bytes,
        })
    }

    /// Range reads for the unified preview transport; authorization stays here.
    pub async fn read_preview_range(
        &self,
        actor: &crate::Actor,
        file_id: &str,
        range: std::ops::Range<u64>,
    ) -> TalosResult<super::FileContent> {
        if range.end.saturating_sub(range.start)
            > u64::from(talos_types::FILE_PREVIEW_MAX_READ_BYTES)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Preview reads are limited to one MiB per request.",
            ));
        }
        let record = self.ready_file(actor, file_id).await?;
        let info = super::service::file_info(record.clone())?;
        let end = range.end.min(info.size_bytes);
        let start = range.start.min(end);
        let bytes = match &record.location {
            talos_store::file::FileLocation::Local {
                storage_id,
                object_key,
            } => {
                if start == end {
                    bytes::Bytes::new()
                } else {
                    self.operator(storage_id)?
                        .read_with(object_key)
                        .range(start..end)
                        .await
                        .map_err(|error| opendal_error("read preview range", error))?
                        .to_bytes()
                }
            }
            talos_store::file::FileLocation::Peer { peer_id } => {
                if start == end {
                    bytes::Bytes::new()
                } else {
                    let content = self
                        .peer_access()?
                        .read_range(
                            &record.owner_user_id,
                            peer_id,
                            talos_types::FileReadRequest {
                                file_id: file_id.to_owned(),
                            },
                            start..end,
                        )
                        .await?;
                    if content.file.file_id != info.file_id
                        || content.file.scope != info.scope
                        || content.file.size_bytes != info.size_bytes
                        || content.bytes.len() as u64 != end - start
                    {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Source file does not match the authorized metadata",
                        ));
                    }
                    content.bytes
                }
            }
        };
        Ok(super::FileContent { file: info, bytes })
    }

    async fn read_attachments(
        &self,
        scope: &TurnFileScope,
        file_ids: &[String],
    ) -> TalosResult<Vec<ResolvedAttachment>> {
        self.validate_attachment_ids(file_ids)?;
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let records = self
            .store
            .get_files_for_owner(&scope.owner_user_id, file_ids)
            .await?;
        let mut records = records
            .into_iter()
            .map(|record| (record.file_id.clone(), record))
            .collect::<HashMap<_, _>>();
        let mut total_bytes = 0_u64;
        let mut authorized = Vec::with_capacity(file_ids.len());
        for file_id in file_ids {
            let record = records
                .remove(file_id)
                .filter(|record| authorized_ready_record(record, scope))
                .ok_or_else(|| file_not_found(file_id))?;
            let expected_size = record.size_bytes.ok_or_else(|| file_not_found(file_id))?;
            let actual_size = match &record.location {
                talos_store::file::FileLocation::Local {
                    storage_id,
                    object_key,
                } => self
                    .operator(storage_id)?
                    .stat(object_key)
                    .await
                    .map_err(|error| opendal_error("stat file object", error))?
                    .content_length(),
                talos_store::file::FileLocation::Peer { .. } => expected_size,
            };
            if actual_size > self.limits.max_upload_bytes {
                return Err(attachment_limit_error(format!(
                    "file {file_id} exceeds the per-file byte limit"
                )));
            }
            total_bytes = total_bytes
                .checked_add(actual_size)
                .ok_or_else(|| attachment_limit_error("file byte count overflow".to_string()))?;
            if total_bytes > self.limits.max_turn_file_bytes {
                return Err(attachment_limit_error(format!(
                    "turn files exceed {} bytes",
                    self.limits.max_turn_file_bytes
                )));
            }
            if actual_size != expected_size {
                return Err(TalosError::internal(
                    "stored file size does not match file metadata",
                ));
            }
            authorized.push((record, actual_size));
        }
        let mut resolved = Vec::with_capacity(authorized.len());
        for (record, size_bytes) in authorized {
            resolved.push(self.read_attachment(record, size_bytes).await?);
        }
        Ok(resolved)
    }

    fn validate_attachment_ids(&self, file_ids: &[String]) -> TalosResult<()> {
        if file_ids.len() > self.limits.max_files_per_turn {
            return Err(TalosError::localized(
                talos_error::ErrorLocalization::AttachmentCountExceeded {
                    limit: self.limits.max_files_per_turn as u64,
                    actual: file_ids.len() as u64,
                },
            ));
        }
        let mut unique = HashSet::with_capacity(file_ids.len());
        if file_ids
            .iter()
            .any(|file_id| file_id.trim().is_empty() || !unique.insert(file_id))
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file ids must be non-empty and unique".to_string(),
            ));
        }
        Ok(())
    }

    async fn read_attachment(
        &self,
        record: FileRecord,
        expected_size: u64,
    ) -> TalosResult<ResolvedAttachment> {
        let media_type = record
            .mime_type
            .clone()
            .ok_or_else(|| file_not_found(&record.file_id))?;
        let bytes = match &record.location {
            talos_store::file::FileLocation::Local {
                storage_id,
                object_key,
            } => self
                .operator(storage_id)?
                .read(object_key)
                .await
                .map_err(|error| opendal_error("read file object", error))?
                .to_bytes(),
            talos_store::file::FileLocation::Peer { peer_id } => {
                let content = self
                    .peer_access()?
                    .read(
                        &record.owner_user_id,
                        peer_id,
                        talos_types::FileReadRequest {
                            file_id: record.file_id.clone(),
                        },
                        self.limits
                            .max_upload_bytes
                            .min(self.limits.max_turn_file_bytes),
                    )
                    .await?;
                if content.file.file_id != record.file_id
                    || content.file.scope
                        != match &record.project_id {
                            Some(project_id) => talos_types::FileScope::Project {
                                project_id: project_id.clone(),
                            },
                            None => talos_types::FileScope::Personal,
                        }
                    || content.file.size_bytes != expected_size
                {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Source file does not match the authorized metadata",
                    ));
                }
                content.bytes
            }
        };
        let actual_size = u64::try_from(bytes.len())
            .map_err(|_| attachment_limit_error("file byte count overflow".to_string()))?;
        if actual_size != expected_size {
            return Err(TalosError::internal(
                "stored file size does not match file metadata",
            ));
        }
        Ok(ResolvedAttachment {
            file_id: record.file_id,
            filename: record.original_filename,
            media_type,
            size_bytes: actual_size,
            bytes,
        })
    }
}

fn authorized_ready_record(record: &FileRecord, scope: &TurnFileScope) -> bool {
    record.status == FileStatus::Ready
        && record
            .project_id
            .as_deref()
            .is_none_or(|project_id| scope.project_id.as_deref() == Some(project_id))
}

fn attachment_limit_error(message: String) -> TalosError {
    TalosError::new(ErrorCode::AttachmentLimitExceeded, message)
}
