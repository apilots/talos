use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono_tz::Tz;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, Sqlite, SqlitePool};
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryBundleEventKind, DreamMemoryDate, DreamMemoryProvenance, DreamMemorySourceRange,
    MemoryScope, MemorySessionId,
};

use super::bundle::current_memory_date;
use super::consistency::{clear_index_stale_marker, mark_index_stale};
use super::source::source_from_markdown;
use super::types::{
    memory_importance_as_str, memory_kind_as_str, MemoryIndexScope, ParsedMemorySource,
};

static SQLITE_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./src/memory/migrations/sqlite");

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
                TalosError::internal_with_source(
                    "create memory index directory; technical details were withheld.",
                    error,
                )
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
                return Err(TalosError::internal(
                    "memory index is corrupt after rebuild",
                ));
            }
            match SQLITE_MIGRATOR.run(&pool).await {
                Ok(_) => {
                    return Ok(Self { pool });
                }
                Err(error) => {
                    return Err(TalosError::internal_with_source(
                        "initialize memory index; technical details were withheld.",
                        error,
                    ));
                }
            }
        }
        Err(TalosError::internal(
            "failed to rebuild corrupt memory index",
        ))
    }

    /// Create an isolated in-memory index for deterministic tests.
    pub async fn in_memory() -> TalosResult<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .map_err(storage_error("connect memory index"))?;
        SQLITE_MIGRATOR.run(&pool).await.map_err(|error| {
            TalosError::internal_with_source(
                "initialize memory index; technical details were withheld.",
                error,
            )
        })?;
        Ok(Self { pool })
    }

    /// Synchronize one user scope from canonical daily Markdown into the derived index.
    pub async fn rebuild_scope(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        timezone: Tz,
    ) -> TalosResult<()> {
        let lock_path = memory_scope_lock_path(&scope.workspace_root);
        talos_fs::with_file_lock(&lock_path, || async {
            self.rebuild_scope_unlocked(scope, memory_dirname, timezone)
                .await
        })
        .await
    }

    pub(super) async fn rebuild_scope_unlocked(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        timezone: Tz,
    ) -> TalosResult<()> {
        let index_scope = MemoryIndexScope::from_scope(scope)?;
        let sources = load_sources(scope, memory_dirname, timezone).await?;
        validate_scope_sources(&sources)?;
        mark_index_stale(&scope.workspace_root).await?;
        self.sync_bundle_unlocked(
            scope,
            memory_dirname,
            &sources,
            DreamMemoryBundleEventKind::Recovery,
            current_memory_date(timezone)?,
        )
        .await?;

        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(storage_error("begin memory rebuild"))?;
        let existing = sqlx::query(
            "SELECT source_path, fingerprint FROM memory_sources WHERE workspace_id = ? AND user_id = ? AND workspace_root = ?",
        )
                .bind(index_scope.workspace_id())
                .bind(index_scope.user_id())
                .bind(index_scope.workspace_root())
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
            sqlx::query("DELETE FROM memory_entries WHERE workspace_id = ? AND user_id = ? AND workspace_root = ? AND source_path = ?")
                .bind(index_scope.workspace_id())
                .bind(index_scope.user_id())
                .bind(index_scope.workspace_root())
                .bind(source_path)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error("delete removed memory entries"))?;
            sqlx::query("DELETE FROM memory_sources WHERE workspace_id = ? AND user_id = ? AND workspace_root = ? AND source_path = ?")
                .bind(index_scope.workspace_id())
                .bind(index_scope.user_id())
                .bind(index_scope.workspace_root())
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
            replace_source(&mut transaction, &index_scope, &source).await?;
        }
        transaction
            .commit()
            .await
            .map_err(storage_error("commit memory rebuild"))?;
        clear_index_stale_marker(&scope.workspace_root)?;
        Ok(())
    }

    /// Replace one parsed source in the derived index after its Markdown file is committed.
    pub(super) async fn sync_source(
        &self,
        scope: &MemoryScope,
        source_path: &str,
        content: &str,
        timezone: Tz,
    ) -> TalosResult<()> {
        let index_scope = MemoryIndexScope::from_scope(scope)?;
        let source = source_from_markdown(
            &scope.workspace_id,
            &scope.user_id,
            source_path,
            content,
            timezone,
        )?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(storage_error("begin memory source sync"))?;
        replace_source(&mut transaction, &index_scope, &source).await?;
        transaction
            .commit()
            .await
            .map_err(storage_error("commit memory source sync"))
    }

    /// Count canonical records in one source using the same parser as rebuild and search.
    pub fn source_section_count(
        &self,
        workspace_id: &str,
        user_id: &str,
        source_date: DreamMemoryDate,
        content: &str,
        timezone: Tz,
    ) -> TalosResult<usize> {
        let source_path = format!("{source_date}.md");
        Ok(
            source_from_markdown(workspace_id, user_id, &source_path, content, timezone)?
                .records
                .len(),
        )
    }
}

pub(super) async fn load_sources(
    scope: &MemoryScope,
    memory_dirname: &str,
    timezone: Tz,
) -> TalosResult<Vec<ParsedMemorySource>> {
    MemoryIndexScope::from_scope(scope)?;
    let memory_dir = scope.workspace_root.join(memory_dirname);
    let Some(entries) = talos_fs::read_directory(&memory_dir)? else {
        return Ok(Vec::new());
    };
    let mut paths = entries
        .into_iter()
        .filter(|entry| entry.metadata.is_file)
        .filter_map(|entry| {
            let path = entry.path;
            let is_dream_memory = path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.len() == 13 && name.ends_with(".md"));
            is_dream_memory.then_some(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "invalid memory filename".to_string(),
                )
            })?;
        let source_path = format!("{memory_dirname}/{name}");
        let content = talos_fs::read_file(&path).await?.ok_or_else(|| {
            TalosError::internal("memory source missing; technical details were withheld.")
                .context(format!("memory source missing: {source_path}"))
        })?;
        sources.push(source_from_markdown(
            &scope.workspace_id,
            &scope.user_id,
            &source_path,
            &content,
            timezone,
        )?);
    }
    Ok(sources)
}

pub(super) fn validate_scope_sources(sources: &[ParsedMemorySource]) -> TalosResult<()> {
    let mut memory_ids = HashSet::new();
    let mut generated = Vec::new();
    for source in sources {
        for parsed in &source.records {
            let record = &parsed.record;
            if !memory_ids.insert(&record.memory_id) {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    format!(
                        "duplicate memory_id {} in user scope",
                        record.memory_id.as_str()
                    ),
                ));
            }
            if let DreamMemoryProvenance::Generated {
                session_id,
                source_range,
            } = &record.provenance
            {
                validate_sqlite_source_range(&record.memory_id, *source_range)?;
                generated.push(GeneratedSourceRange {
                    session_id,
                    source_range: *source_range,
                });
            }
        }
    }
    for (index, range) in generated.iter().enumerate() {
        for other in &generated[index + 1..] {
            if range.session_id == other.session_id
                && range.source_range != other.source_range
                && range.source_range.overlaps(other.source_range)
            {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Dream Memory entries contain overlapping session ranges".to_string(),
                ));
            }
        }
    }
    Ok(())
}

struct GeneratedSourceRange<'a> {
    session_id: &'a MemorySessionId,
    source_range: DreamMemorySourceRange,
}

fn validate_sqlite_source_range(
    memory_id: &talos_memory::MemoryId,
    source_range: DreamMemorySourceRange,
) -> TalosResult<()> {
    if i64::try_from(source_range.from_seq()).is_err()
        || i64::try_from(source_range.until_seq()).is_err()
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "memory {} source range exceeds SQLite integer bounds",
                memory_id.as_str()
            ),
        ));
    }
    Ok(())
}

/// Returns the single lock target for every Dream Memory mutation in one user workspace.
///
/// This is public only for the one-time legacy converter.
/// TODO(dream-memory-okf): Reduce visibility after removing the converter in the next release.
pub fn memory_scope_lock_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".dream-memory-scope")
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
                return Err({
                    let diagnostic = format!(
                        "remove corrupt memory index {}: {error}",
                        candidate.display()
                    );
                    TalosError::internal_with_source(
                        "Remove corrupt memory index; technical details were withheld.",
                        error,
                    )
                    .context(diagnostic)
                });
            }
        }
    }
    Ok(())
}

async fn replace_source(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    scope: &MemoryIndexScope,
    source: &ParsedMemorySource,
) -> TalosResult<()> {
    sqlx::query("DELETE FROM memory_entries WHERE workspace_id = ? AND user_id = ? AND workspace_root = ? AND source_path = ?")
        .bind(scope.workspace_id())
        .bind(scope.user_id())
        .bind(scope.workspace_root())
        .bind(&source.source_path)
        .execute(&mut **transaction)
        .await
        .map_err(storage_error("replace memory source entries"))?;
    for parsed in &source.records {
        let record = &parsed.record;
        let provenance = SqliteProvenance::from_domain(&record.provenance)?;
        sqlx::query(
            "INSERT INTO memory_entries (workspace_id, user_id, workspace_root, memory_id, title, body, kind, importance, occurred_at, session_id, source_from_seq, source_until_seq, source_path, source_date) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(scope.workspace_id())
        .bind(scope.user_id())
        .bind(scope.workspace_root())
        .bind(record.memory_id.as_str())
        .bind(&record.title)
        .bind(&record.body)
        .bind(memory_kind_as_str(record.kind))
        .bind(memory_importance_as_str(record.importance))
        .bind(record.occurred_at)
        .bind(provenance.session_id)
        .bind(provenance.source_from_seq)
        .bind(provenance.source_until_seq)
        .bind(&source.source_path)
        .bind(record.source_date.to_string())
        .execute(&mut **transaction)
        .await
        .map_err(storage_error("insert memory entry"))?;
    }
    sqlx::query(
        "INSERT INTO memory_sources (workspace_id, user_id, workspace_root, source_path, fingerprint) VALUES (?, ?, ?, ?, ?) ON CONFLICT(workspace_id, user_id, workspace_root, source_path) DO UPDATE SET fingerprint = excluded.fingerprint",
    )
    .bind(scope.workspace_id())
    .bind(scope.user_id())
    .bind(scope.workspace_root())
    .bind(&source.source_path)
    .bind(&source.fingerprint)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error("update memory source fingerprint"))?;
    Ok(())
}

struct SqliteProvenance<'a> {
    session_id: Option<&'a str>,
    source_from_seq: Option<i64>,
    source_until_seq: Option<i64>,
}

impl<'a> SqliteProvenance<'a> {
    fn from_domain(provenance: &'a DreamMemoryProvenance) -> TalosResult<Self> {
        match provenance {
            DreamMemoryProvenance::Generated {
                session_id,
                source_range,
            } => Ok(Self {
                session_id: Some(session_id.as_str()),
                source_from_seq: Some(i64::try_from(source_range.from_seq()).map_err(|_| {
                    TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "Dream Memory source range exceeds SQLite integer bounds".to_string(),
                    )
                })?),
                source_until_seq: Some(i64::try_from(source_range.until_seq()).map_err(|_| {
                    TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "Dream Memory source range exceeds SQLite integer bounds".to_string(),
                    )
                })?),
            }),
            DreamMemoryProvenance::UserAuthored => Ok(Self {
                session_id: None,
                source_from_seq: None,
                source_until_seq: None,
            }),
        }
    }
}

pub(super) fn storage_error(action: &'static str) -> impl FnOnce(sqlx::Error) -> TalosError {
    move |error| talos_storage::database_error(action, error)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
