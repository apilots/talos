use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use talos_error::{TalosError, TalosResult};
use talos_types::FileScope;

use super::{
    FileIncompleteCursor, FileListQuery, FileOwnerKey, FileReadyUpdate, FileRecord,
    FileRecordUpdate, FileStatus,
};

const FILE_COLUMNS: &str = "file_id, owner_user_id, project_id, storage_id, object_key, original_filename, mime_type, size_bytes, status, metadata_json, created_at, updated_at";

/// Persists file lifecycle records without exposing SQL or storage objects.
///
/// `FileService` owns authorization and object I/O. Implementations of this
/// interface own database state transitions and owner-scoped lookup semantics.
#[async_trait]
pub trait FileStore: Send + Sync {
    async fn create_file(&self, record: FileRecord) -> TalosResult<()>;

    async fn get_file_for_owner(&self, key: &FileOwnerKey) -> TalosResult<Option<FileRecord>>;

    /// Loads requested metadata in one owner-scoped database query.
    async fn get_files_for_owner(
        &self,
        owner_user_id: &str,
        file_ids: &[String],
    ) -> TalosResult<Vec<FileRecord>>;

    async fn list_files_for_owner(&self, query: FileListQuery) -> TalosResult<Vec<FileRecord>>;

    async fn mark_ready(&self, update: FileReadyUpdate) -> TalosResult<bool>;

    async fn update_file_for_owner(
        &self,
        key: &FileOwnerKey,
        update: FileRecordUpdate,
    ) -> TalosResult<Option<FileRecord>>;

    async fn mark_deleting_for_owner(
        &self,
        key: &FileOwnerKey,
        updated_at: i64,
    ) -> TalosResult<Option<FileRecord>>;

    /// Idempotently removes one incomplete lifecycle record.
    async fn delete_file_record(&self, file_id: &str) -> TalosResult<()>;

    async fn list_incomplete_files(
        &self,
        cursor: Option<FileIncompleteCursor>,
        limit: usize,
    ) -> TalosResult<Vec<FileRecord>>;

    async fn count_by_project(&self, project_id: &str) -> TalosResult<u64>;

    async fn list_storage_ids(&self) -> TalosResult<BTreeSet<String>>;
}

/// SQLite-backed file repository.
pub struct DatabaseFileStore {
    pool: SqlitePool,
}

impl DatabaseFileStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn load_records<'a>(
        &self,
        query: sqlx::query::Query<'a, Sqlite, sqlx::sqlite::SqliteArguments<'a>>,
    ) -> TalosResult<Vec<FileRecord>> {
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|error| storage_error("query files", error))?;
        let mut records = rows
            .iter()
            .map(parse_record)
            .collect::<TalosResult<Vec<_>>>()?;
        self.load_tags(&mut records).await?;
        Ok(records)
    }

    async fn load_tags(&self, records: &mut [FileRecord]) -> TalosResult<()> {
        if records.is_empty() {
            return Ok(());
        }

        let mut builder =
            QueryBuilder::<Sqlite>::new("SELECT file_id, tag FROM file_tags WHERE file_id IN (");
        let mut separated = builder.separated(", ");
        for record in records.iter() {
            separated.push_bind(&record.file_id);
        }
        separated.push_unseparated(") ORDER BY file_id, tag");
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|error| storage_error("query file tags", error))?;

        let mut by_file = BTreeMap::<String, Vec<String>>::new();
        for row in rows {
            let file_id = row
                .try_get::<String, _>("file_id")
                .map_err(|error| storage_error("read file tag id", error))?;
            let tag = row
                .try_get::<String, _>("tag")
                .map_err(|error| storage_error("read file tag", error))?;
            by_file.entry(file_id).or_default().push(tag);
        }
        for record in records {
            record.tags = by_file.remove(&record.file_id).unwrap_or_default();
        }
        Ok(())
    }

    async fn insert_tags(
        transaction: &mut sqlx::Transaction<'_, Sqlite>,
        file_id: &str,
        tags: &[String],
    ) -> TalosResult<()> {
        for tag in tags {
            sqlx::query("INSERT INTO file_tags (file_id, tag) VALUES (?, ?)")
                .bind(file_id)
                .bind(tag)
                .execute(&mut **transaction)
                .await
                .map_err(|error| storage_error("insert file tag", error))?;
        }
        Ok(())
    }
}

#[async_trait]
impl FileStore for DatabaseFileStore {
    async fn create_file(&self, record: FileRecord) -> TalosResult<()> {
        let metadata_json = serde_json::to_string(&record.metadata).map_err(|error| {
            TalosError::internal_with_source(
                "serialize file metadata; technical details were withheld.",
                error,
            )
        })?;
        let size_bytes = record.size_bytes.map(size_to_i64).transpose()?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| storage_error("begin file create", error))?;
        let query = if record.project_id.is_some() {
            sqlx::query(
                r#"
                INSERT INTO files (
                    file_id, owner_user_id, project_id, storage_id, object_key,
                    original_filename, mime_type, size_bytes, status, metadata_json,
                    created_at, updated_at
                )
                SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
                FROM projects
                WHERE project_id = ?
                "#,
            )
        } else {
            sqlx::query(
                r#"
                INSERT INTO files (
                    file_id, owner_user_id, project_id, storage_id, object_key,
                    original_filename, mime_type, size_bytes, status, metadata_json,
                    created_at, updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
        };
        let query = query
            .bind(&record.file_id)
            .bind(&record.owner_user_id)
            .bind(&record.project_id)
            .bind(&record.storage_id)
            .bind(&record.object_key)
            .bind(&record.original_filename)
            .bind(&record.mime_type)
            .bind(size_bytes)
            .bind(record.status.as_str())
            .bind(&metadata_json)
            .bind(record.created_at)
            .bind(record.updated_at);
        let query = match record.project_id.as_ref() {
            Some(project_id) => query.bind(project_id),
            None => query,
        };
        let result = query
            .execute(&mut *transaction)
            .await
            .map_err(|error| storage_error("insert file", error))?;
        if result.rows_affected() == 0 {
            let project_id = record.project_id.as_deref().unwrap_or_default();
            return Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                format!("Project {project_id} was not found."),
            ));
        }
        Self::insert_tags(&mut transaction, &record.file_id, &record.tags).await?;
        transaction
            .commit()
            .await
            .map_err(|error| storage_error("commit file create", error))?;
        Ok(())
    }

    async fn get_file_for_owner(&self, key: &FileOwnerKey) -> TalosResult<Option<FileRecord>> {
        let sql =
            format!("SELECT {FILE_COLUMNS} FROM files WHERE file_id = ? AND owner_user_id = ?");
        Ok(self
            .load_records(
                sqlx::query(&sql)
                    .bind(&key.file_id)
                    .bind(&key.owner_user_id),
            )
            .await?
            .into_iter()
            .next())
    }

    async fn get_files_for_owner(
        &self,
        owner_user_id: &str,
        file_ids: &[String],
    ) -> TalosResult<Vec<FileRecord>> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {FILE_COLUMNS} FROM files WHERE owner_user_id = "
        ));
        builder.push_bind(owner_user_id).push(" AND file_id IN (");
        let mut separated = builder.separated(", ");
        for file_id in file_ids {
            separated.push_bind(file_id);
        }
        separated.push_unseparated(")");
        self.load_records(builder.build()).await
    }

    async fn list_files_for_owner(&self, query: FileListQuery) -> TalosResult<Vec<FileRecord>> {
        validate_limit(query.limit)?;
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {FILE_COLUMNS} FROM files WHERE owner_user_id = "
        ));
        builder.push_bind(&query.owner_user_id);
        builder.push(" AND status = 'ready'");
        match query.scope {
            None => {}
            Some(FileScope::Personal) => {
                builder.push(" AND project_id IS NULL");
            }
            Some(FileScope::Project { project_id }) => {
                builder.push(" AND project_id = ").push_bind(project_id);
            }
        }
        for tag in query.tags {
            builder
                .push(" AND EXISTS (SELECT 1 FROM file_tags WHERE file_tags.file_id = files.file_id AND file_tags.tag = ")
                .push_bind(tag)
                .push(")");
        }
        if let Some(cursor) = query.cursor {
            builder
                .push(" AND (created_at < ")
                .push_bind(cursor.created_at)
                .push(" OR (created_at = ")
                .push_bind(cursor.created_at)
                .push(" AND file_id < ")
                .push_bind(cursor.file_id)
                .push("))");
        }
        builder
            .push(" ORDER BY created_at DESC, file_id DESC LIMIT ")
            .push_bind(limit_to_i64(query.limit)?);
        self.load_records(builder.build()).await
    }

    async fn mark_ready(&self, update: FileReadyUpdate) -> TalosResult<bool> {
        let result = sqlx::query(
            "UPDATE files SET mime_type = ?, size_bytes = ?, status = 'ready', updated_at = ? WHERE file_id = ? AND status = 'uploading'",
        )
        .bind(update.mime_type)
        .bind(size_to_i64(update.size_bytes)?)
        .bind(update.updated_at)
        .bind(update.file_id)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("mark file ready", error))?;
        Ok(result.rows_affected() == 1)
    }

    async fn update_file_for_owner(
        &self,
        key: &FileOwnerKey,
        update: FileRecordUpdate,
    ) -> TalosResult<Option<FileRecord>> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|error| storage_error("begin file update", error))?;
        let metadata_json = update
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "serialize file metadata; technical details were withheld.",
                    error,
                )
            })?;
        let result = match metadata_json {
            Some(metadata_json) => {
                sqlx::query(
                    "UPDATE files SET metadata_json = ?, updated_at = ? WHERE file_id = ? AND owner_user_id = ? AND status = 'ready'",
                )
                .bind(metadata_json)
                .bind(update.updated_at)
                .bind(&key.file_id)
                .bind(&key.owner_user_id)
                .execute(&mut *transaction)
                .await
            }
            None => {
                sqlx::query(
                    "UPDATE files SET updated_at = ? WHERE file_id = ? AND owner_user_id = ? AND status = 'ready'",
                )
                .bind(update.updated_at)
                .bind(&key.file_id)
                .bind(&key.owner_user_id)
                .execute(&mut *transaction)
                .await
            }
        }
        .map_err(|error| storage_error("update file metadata", error))?;
        if result.rows_affected() != 1 {
            transaction
                .rollback()
                .await
                .map_err(|error| storage_error("rollback file update", error))?;
            return Ok(None);
        }
        if let Some(tags) = update.tags {
            sqlx::query("DELETE FROM file_tags WHERE file_id = ?")
                .bind(&key.file_id)
                .execute(&mut *transaction)
                .await
                .map_err(|error| storage_error("clear file tags", error))?;
            Self::insert_tags(&mut transaction, &key.file_id, &tags).await?;
        }
        transaction
            .commit()
            .await
            .map_err(|error| storage_error("commit file update", error))?;
        self.get_file_for_owner(key).await
    }

    async fn mark_deleting_for_owner(
        &self,
        key: &FileOwnerKey,
        updated_at: i64,
    ) -> TalosResult<Option<FileRecord>> {
        sqlx::query(
            "UPDATE files SET status = 'deleting', updated_at = ? WHERE file_id = ? AND owner_user_id = ? AND status = 'ready'",
        )
        .bind(updated_at)
        .bind(&key.file_id)
        .bind(&key.owner_user_id)
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("mark file deleting", error))?;
        Ok(self
            .get_file_for_owner(key)
            .await?
            .filter(|record| record.status == FileStatus::Deleting))
    }

    async fn delete_file_record(&self, file_id: &str) -> TalosResult<()> {
        sqlx::query("DELETE FROM files WHERE file_id = ? AND status IN ('uploading', 'deleting')")
            .bind(file_id)
            .execute(&self.pool)
            .await
            .map_err(|error| storage_error("delete file record", error))?;
        Ok(())
    }

    async fn list_incomplete_files(
        &self,
        cursor: Option<FileIncompleteCursor>,
        limit: usize,
    ) -> TalosResult<Vec<FileRecord>> {
        validate_limit(limit)?;
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {FILE_COLUMNS} FROM files WHERE status IN ('uploading', 'deleting')"
        ));
        if let Some(cursor) = cursor {
            builder
                .push(" AND (updated_at > ")
                .push_bind(cursor.updated_at)
                .push(" OR (updated_at = ")
                .push_bind(cursor.updated_at)
                .push(" AND file_id > ")
                .push_bind(cursor.file_id)
                .push("))");
        }
        builder
            .push(" ORDER BY updated_at, file_id LIMIT ")
            .push_bind(limit_to_i64(limit)?);
        self.load_records(builder.build()).await
    }

    async fn count_by_project(&self, project_id: &str) -> TalosResult<u64> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM files WHERE project_id = ?")
            .bind(project_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| storage_error("count project files", error))?;
        u64::try_from(count).map_err(|_| TalosError::internal("project file count is negative"))
    }

    async fn list_storage_ids(&self) -> TalosResult<BTreeSet<String>> {
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT storage_id FROM files ORDER BY storage_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| storage_error("list file storage ids", error))?;
        Ok(ids.into_iter().collect())
    }
}

fn parse_record(row: &sqlx::sqlite::SqliteRow) -> TalosResult<FileRecord> {
    let status = match row
        .try_get::<String, _>("status")
        .map_err(|error| storage_error("read file status", error))?
        .as_str()
    {
        "uploading" => FileStatus::Uploading,
        "ready" => FileStatus::Ready,
        "deleting" => FileStatus::Deleting,
        status => {
            return Err(TalosError::internal("Unknown file status.")
                .context(format!("unknown file status '{status}'")))
        }
    };
    let metadata_json = row
        .try_get::<String, _>("metadata_json")
        .map_err(|error| storage_error("read file metadata", error))?;
    let metadata = serde_json::from_str(&metadata_json).map_err(|error| {
        TalosError::internal_with_source(
            "parse file metadata; technical details were withheld.",
            error,
        )
    })?;
    let size_bytes = row
        .try_get::<Option<i64>, _>("size_bytes")
        .map_err(|error| storage_error("read file size", error))?
        .map(|size| u64::try_from(size).map_err(|_| TalosError::internal("file size is negative")))
        .transpose()?;
    Ok(FileRecord {
        file_id: read_string(row, "file_id")?,
        owner_user_id: read_string(row, "owner_user_id")?,
        project_id: row
            .try_get("project_id")
            .map_err(|error| storage_error("read file project", error))?,
        storage_id: read_string(row, "storage_id")?,
        object_key: read_string(row, "object_key")?,
        original_filename: read_string(row, "original_filename")?,
        mime_type: row
            .try_get("mime_type")
            .map_err(|error| storage_error("read file MIME type", error))?,
        size_bytes,
        status,
        tags: Vec::new(),
        metadata,
        created_at: row
            .try_get("created_at")
            .map_err(|error| storage_error("read file created time", error))?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|error| storage_error("read file updated time", error))?,
    })
}

fn read_string(row: &sqlx::sqlite::SqliteRow, column: &str) -> TalosResult<String> {
    row.try_get(column)
        .map_err(|error| storage_error(&format!("read file {column}"), error))
}

fn size_to_i64(size: u64) -> TalosResult<i64> {
    i64::try_from(size).map_err(|_| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file size exceeds SQLite range".to_string(),
        )
    })
}

fn validate_limit(limit: usize) -> TalosResult<()> {
    if limit == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file query limit must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

fn limit_to_i64(limit: usize) -> TalosResult<i64> {
    i64::try_from(limit).map_err(|_| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file query limit is too large".to_string(),
        )
    })
}

fn storage_error(context: &str, error: sqlx::Error) -> TalosError {
    talos_storage::database_error(context, error)
}
