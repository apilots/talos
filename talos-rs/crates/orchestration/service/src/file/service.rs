use std::collections::BTreeMap;
use std::sync::Arc;

use opendal::Operator;
use talos_config::FileStorageConfig;
use talos_error::{TalosError, TalosResult};
use talos_store::file::{
    FileIncompleteCursor, FileListCursor, FileListQuery, FileOwnerKey, FileRecord,
    FileRecordUpdate, FileStatus, FileStore,
};
use talos_store::project::ProjectStore;
use talos_types::{
    FileDeleteRequest, FileDeleteResponse, FileInfo, FileListRequest, FileListResponse,
    FileReadRequest, FileReadResponse, FileScope, FileUpdateRequest, FileUpdateResponse,
};

use crate::Actor;

use super::{cursor, validation, FileService};

const DEFAULT_LIST_LIMIT: usize = 50;
const MAX_LIST_LIMIT: usize = 100;
const RECOVERY_BATCH_SIZE: usize = 100;

impl FileService {
    pub fn new(
        store: Arc<dyn FileStore>,
        projects: Arc<dyn ProjectStore>,
        operators: BTreeMap<String, Operator>,
        config: FileStorageConfig,
    ) -> TalosResult<Self> {
        let configured_ids = config.backends.keys().cloned().collect::<Vec<_>>();
        let operator_ids = operators.keys().cloned().collect::<Vec<_>>();
        if configured_ids != operator_ids {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "storage.files backend operators do not match configured backend ids".to_string(),
            ));
        }
        if !operators.contains_key(&config.default) {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "storage.files.default has no OpenDAL operator: {}",
                    config.default
                ),
            ));
        }
        Ok(Self {
            store,
            projects,
            operators,
            default_storage_id: config.default,
            limits: config.limits,
            allowed_mime_types: config.allowed_mime_types,
        })
    }

    /// Returns immutable upload and model-input limits for transport prechecks.
    pub fn limits(&self) -> &talos_config::FileStorageLimits {
        &self.limits
    }

    pub async fn list(
        &self,
        actor: &Actor,
        request: FileListRequest,
    ) -> TalosResult<FileListResponse> {
        let limit = request
            .limit
            .map(|limit| limit as usize)
            .unwrap_or(DEFAULT_LIST_LIMIT);
        if limit == 0 || limit > MAX_LIST_LIMIT {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("file list limit must be between 1 and {MAX_LIST_LIMIT}"),
            ));
        }
        if let Some(scope) = request.scope.as_ref() {
            self.ensure_scope_access(actor, scope).await?;
        }
        let tags = match request.tags {
            None => Vec::new(),
            Some(tags) if tags.is_empty() => {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "file tag filter must not be empty when present".to_string(),
                ))
            }
            Some(tags) => validation::tags(tags, &self.limits)?,
        };
        let cursor = request.cursor.as_deref().map(cursor::decode).transpose()?;
        let records = self
            .store
            .list_files_for_owner(FileListQuery {
                owner_user_id: actor.user_id.clone(),
                scope: request.scope,
                tags,
                cursor,
                limit: limit + 1,
            })
            .await?;
        let has_next = records.len() > limit;
        let data = records
            .into_iter()
            .take(limit)
            .map(file_info)
            .collect::<TalosResult<Vec<_>>>()?;
        let next_cursor = if has_next {
            data.last()
                .map(|file| {
                    cursor::encode(&FileListCursor {
                        created_at: file.created_at,
                        file_id: file.file_id.clone(),
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(FileListResponse { data, next_cursor })
    }

    pub async fn read(
        &self,
        actor: &Actor,
        request: FileReadRequest,
    ) -> TalosResult<FileReadResponse> {
        let record = self.ready_file(actor, &request.file_id).await?;
        Ok(FileReadResponse {
            file: file_info(record)?,
        })
    }

    pub async fn update(
        &self,
        actor: &Actor,
        request: FileUpdateRequest,
    ) -> TalosResult<FileUpdateResponse> {
        if request.tags.is_none() && request.metadata.is_none() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file update must include tags or metadata".to_string(),
            ));
        }
        let tags = request
            .tags
            .map(|tags| validation::tags(tags, &self.limits))
            .transpose()?;
        if let Some(metadata) = request.metadata.as_ref() {
            validation::metadata(metadata, &self.limits)?;
        }
        let key = owner_key(actor, &request.file_id);
        let record = self
            .store
            .update_file_for_owner(
                &key,
                FileRecordUpdate {
                    tags,
                    metadata: request.metadata,
                    updated_at: chrono::Utc::now().timestamp(),
                },
            )
            .await?
            .ok_or_else(|| file_not_found(&request.file_id))?;
        Ok(FileUpdateResponse {
            file: file_info(record)?,
        })
    }

    pub async fn delete(
        &self,
        actor: &Actor,
        request: FileDeleteRequest,
    ) -> TalosResult<FileDeleteResponse> {
        let key = owner_key(actor, &request.file_id);
        let record = self
            .store
            .mark_deleting_for_owner(&key, chrono::Utc::now().timestamp())
            .await?
            .ok_or_else(|| file_not_found(&request.file_id))?;
        self.delete_object_and_record(&record).await?;
        Ok(FileDeleteResponse {
            file_id: request.file_id,
        })
    }

    /// Removes interrupted uploads and deletions before gateways accept traffic.
    pub async fn reconcile_incomplete(&self) -> TalosResult<()> {
        self.validate_stored_backends().await?;
        let mut cursor = None;
        loop {
            let records = self
                .store
                .list_incomplete_files(cursor, RECOVERY_BATCH_SIZE)
                .await?;
            if records.is_empty() {
                return Ok(());
            }
            for record in &records {
                self.delete_object_and_record(record).await?;
            }
            cursor = records.last().map(|record| FileIncompleteCursor {
                updated_at: record.updated_at,
                file_id: record.file_id.clone(),
            });
        }
    }

    /// Rejects project deletion while any file record still references it.
    pub async fn ensure_project_has_no_files(&self, project_id: &str) -> TalosResult<()> {
        if self.store.count_by_project(project_id).await? != 0 {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                format!("project {project_id} still has files"),
            ));
        }
        Ok(())
    }

    pub(super) async fn ensure_scope_access(
        &self,
        actor: &Actor,
        scope: &FileScope,
    ) -> TalosResult<()> {
        let FileScope::Project { project_id } = scope else {
            return Ok(());
        };
        self.projects
            .get_project(project_id)
            .await?
            .filter(|project| project.user_id == actor.user_id)
            .map(|_| ())
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("Project {project_id} was not found."),
                )
            })
    }

    pub(super) fn operator(&self, storage_id: &str) -> TalosResult<&Operator> {
        self.operators.get(storage_id).ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file record references an unconfigured storage backend".to_string(),
            )
        })
    }

    async fn ready_file(&self, actor: &Actor, file_id: &str) -> TalosResult<FileRecord> {
        self.store
            .get_file_for_owner(&owner_key(actor, file_id))
            .await?
            .filter(|record| record.status == FileStatus::Ready)
            .ok_or_else(|| file_not_found(file_id))
    }

    async fn validate_stored_backends(&self) -> TalosResult<()> {
        for storage_id in self.store.list_storage_ids().await? {
            let _ = self.operator(&storage_id)?;
        }
        Ok(())
    }

    async fn delete_object_and_record(&self, record: &FileRecord) -> TalosResult<()> {
        self.operator(&record.storage_id)?
            .delete(&record.object_key)
            .await
            .map_err(|error| opendal_error("delete file object", error))?;
        self.store.delete_file_record(&record.file_id).await?;
        Ok(())
    }
}

fn owner_key(actor: &Actor, file_id: &str) -> FileOwnerKey {
    FileOwnerKey {
        file_id: file_id.to_string(),
        owner_user_id: actor.user_id.clone(),
    }
}

pub(super) fn file_not_found(file_id: &str) -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::ResourceNotFound,
        format!("File {file_id} was not found."),
    )
}

pub(super) fn file_info(record: FileRecord) -> TalosResult<FileInfo> {
    if record.status != FileStatus::Ready {
        return Err(TalosError::internal("File is not ready.")
            .context(format!("file {} is not ready", record.file_id)));
    }
    let mime_type = record.mime_type.ok_or_else(|| {
        TalosError::internal("Ready file has no MIME type.")
            .context(format!("ready file {} has no MIME type", record.file_id))
    })?;
    let size_bytes = record.size_bytes.ok_or_else(|| {
        TalosError::internal("Ready file has no size.")
            .context(format!("ready file {} has no size", record.file_id))
    })?;
    Ok(FileInfo {
        file_id: record.file_id,
        scope: match record.project_id {
            Some(project_id) => FileScope::Project { project_id },
            None => FileScope::Personal,
        },
        original_filename: record.original_filename,
        mime_type,
        size_bytes,
        tags: record.tags,
        metadata: record.metadata,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

pub(super) fn opendal_error(context: &str, error: opendal::Error) -> TalosError {
    TalosError::internal_with_source(format!("Failed to {context}: {}.", error.kind()), error)
}
