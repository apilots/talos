use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono_tz::Tz;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use talos_error::{TalosError, TalosResult};

use super::parser::parse_source;
use super::types::{memory_importance_as_str, memory_kind_as_str, ParsedMemorySource};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS memory_entries (
    row_id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    kind TEXT NOT NULL,
    importance TEXT NOT NULL,
    occurred_at INTEGER NOT NULL,
    session_id TEXT,
    source_from_seq INTEGER,
    source_until_seq INTEGER,
    source_path TEXT NOT NULL,
    source_date TEXT NOT NULL,
    UNIQUE(user_id, memory_id)
);
CREATE INDEX IF NOT EXISTS idx_memory_entries_user_time
    ON memory_entries(user_id, occurred_at DESC, memory_id);
CREATE INDEX IF NOT EXISTS idx_memory_entries_user_source
    ON memory_entries(user_id, source_path);
CREATE VIRTUAL TABLE IF NOT EXISTS memory_entries_fts USING fts5(
    title,
    body,
    content='memory_entries',
    content_rowid='row_id',
    tokenize='trigram'
);
CREATE TRIGGER IF NOT EXISTS memory_entries_ai AFTER INSERT ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(rowid, title, body)
    VALUES (new.row_id, new.title, new.body);
END;
CREATE TRIGGER IF NOT EXISTS memory_entries_ad AFTER DELETE ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(memory_entries_fts, rowid, title, body)
    VALUES ('delete', old.row_id, old.title, old.body);
END;
CREATE TRIGGER IF NOT EXISTS memory_entries_au AFTER UPDATE ON memory_entries BEGIN
    INSERT INTO memory_entries_fts(memory_entries_fts, rowid, title, body)
    VALUES ('delete', old.row_id, old.title, old.body);
    INSERT INTO memory_entries_fts(rowid, title, body)
    VALUES (new.row_id, new.title, new.body);
END;
CREATE TABLE IF NOT EXISTS memory_sources (
    user_id TEXT NOT NULL,
    source_path TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY(user_id, source_path)
);
"#;

/// SQLite-backed derived index for canonical daily Markdown memory files.
#[derive(Clone)]
pub struct MemoryStore {
    pub(super) pool: SqlitePool,
}

impl MemoryStore {
    /// Open the rebuildable on-disk index, replacing only a corrupt SQLite file.
    pub async fn open(path: &Path) -> TalosResult<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                TalosError::StorageError(format!("create memory index directory: {error}"))
            })?;
        }
        for attempt in 0..2 {
            let pool = SqlitePoolOptions::new()
                .max_connections(4)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(path)
                        .create_if_missing(true),
                )
                .await;
            let pool = match pool {
                Ok(pool) => pool,
                Err(error) if attempt == 0 && is_sqlite_corruption(&error) => {
                    remove_index_files(path).await?;
                    continue;
                }
                Err(error) => return Err(storage_error("connect memory index")(error)),
            };
            let quick_check = sqlx::query_scalar::<_, String>("PRAGMA quick_check")
                .fetch_all(&pool)
                .await;
            let healthy = match quick_check {
                Ok(rows) => rows.iter().all(|row| row == "ok"),
                Err(error) if attempt == 0 && is_sqlite_corruption(&error) => false,
                Err(error) => return Err(storage_error("check memory index")(error)),
            };
            if !healthy {
                pool.close().await;
                if attempt == 0 {
                    remove_index_files(path).await?;
                    continue;
                }
                return Err(TalosError::StorageError(
                    "memory index is corrupt after rebuild".to_string(),
                ));
            }
            match sqlx::raw_sql(SCHEMA).execute(&pool).await {
                Ok(_) => {
                    return Ok(Self { pool });
                }
                Err(error) if attempt == 0 && is_sqlite_corruption(&error) => {
                    pool.close().await;
                    remove_index_files(path).await?;
                }
                Err(error) => return Err(storage_error("initialize memory index")(error)),
            }
        }
        Err(TalosError::StorageError(
            "failed to rebuild corrupt memory index".to_string(),
        ))
    }

    /// Create an isolated in-memory index for deterministic tests.
    pub async fn in_memory() -> TalosResult<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .map_err(storage_error("connect memory index"))?;
        sqlx::raw_sql(SCHEMA)
            .execute(&pool)
            .await
            .map_err(storage_error("initialize memory index"))?;
        Ok(Self { pool })
    }

    /// Synchronize one user scope from canonical daily Markdown into the derived index.
    pub async fn rebuild_scope(
        &self,
        user_id: &str,
        workspace_root: &Path,
        memory_dirname: &str,
        timezone: Tz,
    ) -> TalosResult<()> {
        if user_id.trim().is_empty() {
            return Err(TalosError::ValidationError(
                "memory user_id cannot be empty".to_string(),
            ));
        }
        let sources = load_sources(user_id, workspace_root, memory_dirname, timezone).await?;
        let mut memory_ids = HashSet::new();
        for source in &sources {
            for parsed in &source.records {
                if !memory_ids.insert(parsed.record.memory_id.as_str()) {
                    return Err(TalosError::ValidationError(format!(
                        "duplicate memory_id {} in user scope",
                        parsed.record.memory_id
                    )));
                }
            }
        }

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(storage_error("begin memory rebuild"))?;
        let existing =
            sqlx::query("SELECT source_path, fingerprint FROM memory_sources WHERE user_id = ?")
                .bind(user_id)
                .fetch_all(&mut *transaction)
                .await
                .map_err(storage_error("load memory source fingerprints"))?
                .into_iter()
                .map(|row| {
                    Ok((
                        row.try_get::<String, _>("source_path")?,
                        row.try_get::<String, _>("fingerprint")?,
                    ))
                })
                .collect::<Result<HashMap<_, _>, sqlx::Error>>()
                .map_err(storage_error("decode memory source fingerprints"))?;

        let current_paths = sources
            .iter()
            .map(|source| source.source_path.as_str())
            .collect::<HashSet<_>>();
        for source_path in existing.keys() {
            if current_paths.contains(source_path.as_str()) {
                continue;
            }
            sqlx::query("DELETE FROM memory_entries WHERE user_id = ? AND source_path = ?")
                .bind(user_id)
                .bind(source_path)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error("delete removed memory entries"))?;
            sqlx::query("DELETE FROM memory_sources WHERE user_id = ? AND source_path = ?")
                .bind(user_id)
                .bind(source_path)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error("delete removed memory source"))?;
        }

        for source in sources {
            if existing
                .get(&source.source_path)
                .is_some_and(|fingerprint| fingerprint == &source.fingerprint)
            {
                continue;
            }
            replace_source(&mut transaction, user_id, &source).await?;
        }
        transaction
            .commit()
            .await
            .map_err(storage_error("commit memory rebuild"))
    }

    /// Validate a whole-file edit without exposing store records to callers.
    pub async fn validate_source_edit(
        &self,
        user_id: &str,
        source_path: &str,
        current: Option<&str>,
        updated: &str,
        timezone: Tz,
    ) -> TalosResult<()> {
        let current = current
            .map(|content| parse_source(user_id, source_path, content, timezone))
            .transpose()?;
        let updated = parse_source(user_id, source_path, updated, timezone)?;
        let current_by_id = current
            .as_ref()
            .into_iter()
            .flat_map(|source| &source.records)
            .map(|parsed| (parsed.record.memory_id.as_str(), &parsed.record))
            .collect::<HashMap<_, _>>();
        let mut memory_ids = HashSet::new();
        for parsed in &updated.records {
            if !memory_ids.insert(parsed.record.memory_id.clone()) {
                return Err(TalosError::Conflict(format!(
                    "duplicate memory_id {} in user scope",
                    parsed.record.memory_id
                )));
            }
        }
        let mut other_source_ids = HashSet::new();
        for ids in memory_ids.iter().collect::<Vec<_>>().chunks(500) {
            let mut query = QueryBuilder::<Sqlite>::new(
                "SELECT memory_id FROM memory_entries WHERE user_id = ",
            );
            query
                .push_bind(user_id)
                .push(" AND source_path <> ")
                .push_bind(source_path)
                .push(" AND memory_id IN (");
            let mut separated = query.separated(", ");
            for memory_id in ids {
                separated.push_bind(memory_id);
            }
            separated.push_unseparated(")");
            other_source_ids.extend(
                query
                    .build_query_scalar::<String>()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(storage_error("validate memory id uniqueness"))?,
            );
        }
        for parsed in &updated.records {
            let record = &parsed.record;
            if other_source_ids.contains(&record.memory_id) {
                return Err(TalosError::Conflict(format!(
                    "duplicate memory_id {} in user scope",
                    record.memory_id
                )));
            }
            let Some(existing) = current_by_id.get(record.memory_id.as_str()) else {
                if record.source_from_seq.is_some() {
                    return Err(TalosError::Conflict(
                        "new generated memory blocks must use the memory writer".to_string(),
                    ));
                }
                continue;
            };
            if existing.occurred_at != record.occurred_at
                || existing.session_id != record.session_id
                || existing.source_from_seq != record.source_from_seq
                || existing.source_until_seq != record.source_until_seq
            {
                return Err(TalosError::Conflict(format!(
                    "memory provenance is immutable for {}",
                    record.memory_id
                )));
            }
        }
        Ok(())
    }

    /// Replace one parsed source in the derived index after its Markdown file is committed.
    pub async fn sync_source(
        &self,
        user_id: &str,
        source_path: &str,
        content: &str,
        timezone: Tz,
    ) -> TalosResult<()> {
        let source = parse_source(user_id, source_path, content, timezone)?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(storage_error("begin memory source sync"))?;
        replace_source(&mut transaction, user_id, &source).await?;
        transaction
            .commit()
            .await
            .map_err(storage_error("commit memory source sync"))
    }

    /// Count canonical records in one source using the same parser as rebuild and search.
    pub fn source_section_count(
        &self,
        user_id: &str,
        source_path: &str,
        content: &str,
        timezone: Tz,
    ) -> TalosResult<usize> {
        Ok(parse_source(user_id, source_path, content, timezone)?
            .records
            .len())
    }

    pub(super) async fn validate_batch_append(
        &self,
        user_id: &str,
        source_path: &str,
        session_id: &str,
        source_from_seq: u64,
        source_until_seq: u64,
        memory_ids: &[String],
    ) -> TalosResult<()> {
        let source_from_seq = i64::try_from(source_from_seq).map_err(|_| {
            TalosError::ValidationError("memory source range exceeds SQLite bounds".to_string())
        })?;
        let source_until_seq = i64::try_from(source_until_seq).map_err(|_| {
            TalosError::ValidationError("memory source range exceeds SQLite bounds".to_string())
        })?;
        let overlapping = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM memory_entries WHERE user_id = ? AND session_id = ? AND source_from_seq <= ? AND source_until_seq >= ? AND NOT (source_path = ? AND source_from_seq = ? AND source_until_seq = ?)",
        )
        .bind(user_id)
        .bind(session_id)
        .bind(source_until_seq)
        .bind(source_from_seq)
        .bind(source_path)
        .bind(source_from_seq)
        .bind(source_until_seq)
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error("validate memory source range"))?;
        if overlapping > 0 {
            return Err(TalosError::Conflict(
                "memory write batch overlaps an existing session range".to_string(),
            ));
        }
        for memory_id in memory_ids {
            let conflicting_source = sqlx::query_scalar::<_, String>(
                "SELECT source_path FROM memory_entries WHERE user_id = ? AND memory_id = ? AND source_path <> ? LIMIT 1",
            )
            .bind(user_id)
            .bind(memory_id)
            .bind(source_path)
            .fetch_optional(&self.pool)
            .await
            .map_err(storage_error("validate generated memory id uniqueness"))?;
            if conflicting_source.is_some() {
                return Err(TalosError::Conflict(format!(
                    "duplicate memory_id {memory_id} in user scope"
                )));
            }
        }
        Ok(())
    }
}

pub(super) async fn load_sources(
    user_id: &str,
    workspace_root: &Path,
    memory_dirname: &str,
    timezone: Tz,
) -> TalosResult<Vec<ParsedMemorySource>> {
    let memory_dir = workspace_root.join(memory_dirname);
    let Some(entries) = talos_fs::read_directory(&memory_dir)? else {
        return Ok(Vec::new());
    };
    let mut paths = entries
        .into_iter()
        .filter(|entry| entry.metadata.is_file)
        .filter_map(|entry| {
            let path = entry.path;
            let is_daily_memory = path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.len() == 13 && name.ends_with(".md"));
            is_daily_memory.then_some(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| TalosError::ValidationError("invalid memory filename".to_string()))?;
        let source_path = format!("{memory_dirname}/{name}");
        let content = talos_fs::read_file(&path).await?.ok_or_else(|| {
            TalosError::StorageError(format!("memory source missing: {source_path}"))
        })?;
        sources.push(parse_source(user_id, &source_path, &content, timezone)?);
    }
    Ok(sources)
}

fn is_sqlite_corruption(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database)
            if matches!(database.code().as_deref(), Some("11" | "26"))
    )
}

async fn remove_index_files(path: &Path) -> TalosResult<()> {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        match tokio::fs::remove_file(&candidate).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(TalosError::StorageError(format!(
                    "remove corrupt memory index {}: {error}",
                    candidate.display()
                )));
            }
        }
    }
    Ok(())
}

async fn replace_source(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    user_id: &str,
    source: &ParsedMemorySource,
) -> TalosResult<()> {
    sqlx::query("DELETE FROM memory_entries WHERE user_id = ? AND source_path = ?")
        .bind(user_id)
        .bind(&source.source_path)
        .execute(&mut **transaction)
        .await
        .map_err(storage_error("replace memory source entries"))?;
    for parsed in &source.records {
        let record = &parsed.record;
        let source_from_seq = record
            .source_from_seq
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                TalosError::ValidationError(format!(
                    "memory {} source_from_seq exceeds SQLite integer bounds",
                    record.memory_id
                ))
            })?;
        let source_until_seq = record
            .source_until_seq
            .map(i64::try_from)
            .transpose()
            .map_err(|_| {
                TalosError::ValidationError(format!(
                    "memory {} source_until_seq exceeds SQLite integer bounds",
                    record.memory_id
                ))
            })?;
        sqlx::query(
            "INSERT INTO memory_entries (user_id, memory_id, title, body, kind, importance, occurred_at, session_id, source_from_seq, source_until_seq, source_path, source_date) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(&record.memory_id)
        .bind(&record.title)
        .bind(&record.body)
        .bind(memory_kind_as_str(record.kind))
        .bind(memory_importance_as_str(record.importance))
        .bind(record.occurred_at)
        .bind(&record.session_id)
        .bind(source_from_seq)
        .bind(source_until_seq)
        .bind(&parsed.source_path)
        .bind(&record.source_date)
        .execute(&mut **transaction)
        .await
        .map_err(storage_error("insert memory entry"))?;
    }
    sqlx::query(
        "INSERT INTO memory_sources (user_id, source_path, fingerprint) VALUES (?, ?, ?) ON CONFLICT(user_id, source_path) DO UPDATE SET fingerprint = excluded.fingerprint",
    )
    .bind(user_id)
    .bind(&source.source_path)
    .bind(&source.fingerprint)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error("update memory source fingerprint"))?;
    Ok(())
}

pub(super) fn storage_error(action: &'static str) -> impl FnOnce(sqlx::Error) -> TalosError {
    move |error| TalosError::StorageError(format!("{action}: {error}"))
}
