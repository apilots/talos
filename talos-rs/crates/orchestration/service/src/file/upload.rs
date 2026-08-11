use futures_util::StreamExt;
use opendal::Writer;
use talos_error::{TalosError, TalosFileErrorKind, TalosResult};
use talos_store::file::{FileReadyUpdate, FileRecord, FileStatus};
use talos_types::{FileUploadRequest, FileUploadResponse};

use crate::Actor;

use super::{
    service::{file_info, opendal_error},
    validation, FileByteStream, FileService,
};

impl FileService {
    pub async fn upload(
        &self,
        actor: &Actor,
        request: FileUploadRequest,
        mut body: FileByteStream,
    ) -> TalosResult<FileUploadResponse> {
        self.ensure_scope_access(actor, &request.scope).await?;
        let original_filename = validation::filename(request.filename, &self.limits)?;
        let tags = validation::tags(request.tags, &self.limits)?;
        validation::metadata(&request.metadata, &self.limits)?;
        let mut media_type_validator = validation::MediaTypeValidator::new(&original_filename);

        let file_id = uuid::Uuid::now_v7().to_string();
        let object_key = format!(
            "{file_id}/content{}",
            validation::normalized_suffix(&original_filename)
        );
        let now = chrono::Utc::now().timestamp();
        let record = FileRecord {
            file_id: file_id.clone(),
            owner_user_id: actor.user_id.clone(),
            project_id: match request.scope {
                talos_types::FileScope::Personal => None,
                talos_types::FileScope::Project { project_id } => Some(project_id),
            },
            storage_id: self.default_storage_id.clone(),
            object_key,
            original_filename,
            mime_type: None,
            size_bytes: None,
            status: FileStatus::Uploading,
            tags,
            metadata: request.metadata,
            created_at: now,
            updated_at: now,
        };
        self.store.create_file(record.clone()).await?;

        let operator = self.operator(&record.storage_id)?;
        let mut writer = match operator.writer(&record.object_key).await {
            Ok(writer) => writer,
            Err(error) => {
                return self
                    .fail_upload(&record, None, opendal_error("create file writer", &error))
                    .await
            }
        };
        let mut size_bytes = 0_u64;
        while let Some(chunk) = body.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    return self
                        .fail_upload(
                            &record,
                            Some(&mut writer),
                            TalosError::internal(format!("read file upload body: {error}")),
                        )
                        .await
                }
            };
            let Some(next_size) = size_bytes.checked_add(chunk.len() as u64) else {
                return self
                    .fail_upload(
                        &record,
                        Some(&mut writer),
                        TalosError::file_error(
                            TalosFileErrorKind::UploadTooLarge,
                            "file upload size overflow",
                        ),
                    )
                    .await;
            };
            size_bytes = next_size;
            if size_bytes > self.limits.max_upload_bytes {
                return self
                    .fail_upload(
                        &record,
                        Some(&mut writer),
                        TalosError::file_error(
                            TalosFileErrorKind::UploadTooLarge,
                            format!("file upload exceeds {} bytes", self.limits.max_upload_bytes),
                        ),
                    )
                    .await;
            }
            media_type_validator.observe(&chunk);
            if let Err(error) = writer.write(chunk).await {
                return self
                    .fail_upload(
                        &record,
                        Some(&mut writer),
                        opendal_error("write file object", &error),
                    )
                    .await;
            }
        }

        let mime_type = match media_type_validator.detected_mime_type() {
            Some(mime_type) if self.allowed_mime_types.contains(&mime_type) => mime_type,
            detected => {
                let message = detected
                    .map(|mime_type| format!("unsupported detected MIME type: {mime_type}"))
                    .unwrap_or_else(|| "file MIME type could not be detected".to_string());
                return self
                    .fail_upload(
                        &record,
                        Some(&mut writer),
                        TalosError::file_error(TalosFileErrorKind::UnsupportedMediaType, message),
                    )
                    .await;
            }
        };
        if let Err(error) = writer.close().await {
            return self
                .fail_upload(
                    &record,
                    Some(&mut writer),
                    opendal_error("close file object", &error),
                )
                .await;
        }
        let ready_at = chrono::Utc::now().timestamp();
        match self
            .store
            .mark_ready(FileReadyUpdate {
                file_id: file_id.clone(),
                mime_type: mime_type.clone(),
                size_bytes,
                updated_at: ready_at,
            })
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                return self
                    .fail_upload(
                        &record,
                        None,
                        TalosError::internal(format!(
                            "mark file ready returned no row for {file_id}"
                        )),
                    )
                    .await
            }
            Err(error) => return self.fail_upload(&record, None, error).await,
        }
        let record = FileRecord {
            mime_type: Some(mime_type),
            size_bytes: Some(size_bytes),
            status: FileStatus::Ready,
            updated_at: ready_at,
            ..record
        };
        Ok(FileUploadResponse {
            file: file_info(record)?,
        })
    }

    async fn fail_upload<T>(
        &self,
        record: &FileRecord,
        writer: Option<&mut Writer>,
        upload_error: TalosError,
    ) -> TalosResult<T> {
        if let Err(cleanup_error) = self.cleanup_upload(record, writer).await {
            return Err(TalosError::internal(format!(
                "file upload failed: {upload_error}; cleanup failed: {cleanup_error}"
            )));
        }
        Err(upload_error)
    }

    async fn cleanup_upload(
        &self,
        record: &FileRecord,
        writer: Option<&mut Writer>,
    ) -> TalosResult<()> {
        let mut cleanup_error = None;
        if let Some(writer) = writer {
            if let Err(error) = writer.abort().await {
                // The filesystem backend without atomic writes does not implement
                // abort; deleting the object below is its supported cleanup path.
                if error.kind() != opendal::ErrorKind::Unsupported {
                    cleanup_error = Some(opendal_error("abort failed file writer", &error));
                }
            }
        }
        if let Err(error) = self
            .operator(&record.storage_id)?
            .delete(&record.object_key)
            .await
        {
            if cleanup_error.is_none() {
                cleanup_error = Some(opendal_error("delete failed file object", &error));
            }
        }
        if let Some(error) = cleanup_error {
            return Err(error);
        }
        self.store.delete_file_record(&record.file_id).await?;
        Ok(())
    }
}
