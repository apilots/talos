use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use talos_agent_kernel::{AttachmentReader, ResolvedAttachment, TurnFileScope};
use talos_error::{TalosError, TalosFileErrorKind, TalosResult};
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
            let actual_size = self
                .operator(&record.storage_id)?
                .stat(&record.object_key)
                .await
                .map_err(|error| opendal_error("stat file object", &error))?
                .content_length();
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
                    "stored file size does not match file metadata".to_string(),
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
            return Err(attachment_limit_error(format!(
                "turn file count exceeds {}",
                self.limits.max_files_per_turn
            )));
        }
        let mut unique = HashSet::with_capacity(file_ids.len());
        if file_ids
            .iter()
            .any(|file_id| file_id.trim().is_empty() || !unique.insert(file_id))
        {
            return Err(TalosError::public(
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
        let buffer = self
            .operator(&record.storage_id)?
            .read(&record.object_key)
            .await
            .map_err(|error| opendal_error("read file object", &error))?;
        let bytes = buffer.to_bytes();
        let actual_size = u64::try_from(bytes.len())
            .map_err(|_| attachment_limit_error("file byte count overflow".to_string()))?;
        if actual_size != expected_size {
            return Err(TalosError::internal(
                "stored file size does not match file metadata".to_string(),
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
    TalosError::file_error(TalosFileErrorKind::AttachmentLimitExceeded, message)
}
