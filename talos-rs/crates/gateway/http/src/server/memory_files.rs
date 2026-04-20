use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, MemoryIndexFile, MemorySearchQuery, MemoryStore};
use talos_types::{DocUpdateRequest, MemoryFileDetail, MemoryFileSummary};
use tokio::fs;

use super::errors::ApiError;
use super::file_support::{
    content_hash, modified_at_ms, read_optional, validate_expected_version, with_file_lock,
    workspace_root, write_atomic,
};
use super::state::HttpApiState;

#[derive(Debug, Deserialize, Default)]
pub(super) struct MemoryFilesQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before_date: Option<String>,
    #[serde(default)]
    date_from: Option<String>,
    #[serde(default)]
    date_to: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct MemorySearchHttpQuery {
    q: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    date_from: Option<String>,
    #[serde(default)]
    date_to: Option<String>,
}

fn memory_dir(state: &HttpApiState) -> PathBuf {
    workspace_root(&state.base_config).join(&state.base_config.agent.kernel.memory.memory_dirname)
}

fn is_memory_date(date: &str) -> bool {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
}

fn parse_memory_date(date: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "memory date must use YYYY-MM-DD",
        )
    })
}

fn memory_file_path(state: &HttpApiState, date: &str) -> Result<PathBuf, ApiError> {
    if !is_memory_date(date) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "memory date must use YYYY-MM-DD",
        ));
    }
    Ok(memory_dir(state).join(format!("{date}.md")))
}

fn memory_index_path(state: &HttpApiState, date: &str) -> Result<PathBuf, ApiError> {
    Ok(memory_file_path(state, date)?.with_extension("index.json"))
}

fn date_in_range(
    date: NaiveDate,
    date_from: Option<NaiveDate>,
    date_to: Option<NaiveDate>,
    before_date: Option<NaiveDate>,
) -> bool {
    if date_from.is_some_and(|from| date < from) {
        return false;
    }
    if date_to.is_some_and(|to| date > to) {
        return false;
    }
    if before_date.is_some_and(|before| date >= before) {
        return false;
    }
    true
}

async fn load_memory_detail(
    state: &HttpApiState,
    date: &str,
) -> Result<Option<MemoryFileDetail>, ApiError> {
    let path = memory_file_path(state, date)?;
    let content = read_optional(&path).await.map_err(map_internal_error)?;
    let updated_at = modified_at_ms(&path).await.map_err(map_internal_error)?;
    match content {
        Some(content) => {
            let version_token = content_hash(&content);
            let section_count = load_memory_section_count(state, date, &version_token).await?;
            Ok(Some(MemoryFileDetail {
                date: date.to_string(),
                path: format!("memory/{date}.md"),
                section_count,
                version_token,
                updated_at,
                content,
            }))
        }
        None => Ok(None),
    }
}

async fn load_memory_section_count(
    state: &HttpApiState,
    date: &str,
    version_token: &str,
) -> Result<usize, ApiError> {
    let index_path = memory_index_path(state, date)?;
    let Some(index_content) = read_optional(&index_path)
        .await
        .map_err(map_internal_error)?
    else {
        return Ok(0);
    };
    let parsed = serde_json::from_str::<MemoryIndexFile>(&index_content).map_err(|err| {
        map_internal_error(TalosError::StorageError(format!(
            "parse memory index {}: {}",
            index_path.display(),
            err
        )))
    })?;
    if parsed.source_content_hash != version_token {
        return Ok(0);
    }
    Ok(parsed.sections.len())
}

fn map_internal_error(err: TalosError) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        err.to_string(),
    )
}

fn map_write_error(err: TalosError) -> ApiError {
    match err {
        TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
        }
        other => map_internal_error(other),
    }
}

async fn restore_previous_memory_file(
    path: &std::path::Path,
    previous_content: Option<&str>,
) -> TalosResult<()> {
    match previous_content {
        Some(content) => write_atomic(path, content).await,
        None => match fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(TalosError::StorageError(format!(
                "remove file during rollback {}: {}",
                path.display(),
                err
            ))),
        },
    }
}

async fn write_memory_file_transaction<F, Fut>(
    path: &std::path::Path,
    content: &str,
    previous_content: Option<&str>,
    rebuild_indexes: &F,
) -> TalosResult<()>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = TalosResult<()>>,
{
    write_atomic(path, content).await?;
    if let Err(rebuild_error) = rebuild_indexes().await {
        let rollback_result = restore_previous_memory_file(path, previous_content).await;
        let reindex_after_rollback = if rollback_result.is_ok() {
            rebuild_indexes().await
        } else {
            Ok(())
        };
        return Err(match (rollback_result, reindex_after_rollback) {
            (Ok(()), Ok(())) => TalosError::StorageError(format!(
                "rebuild memory indexes after write: {}; rollback restored previous content",
                rebuild_error
            )),
            (Ok(()), Err(reindex_error)) => TalosError::StorageError(format!(
                "rebuild memory indexes after write: {}; rollback restored file but reindex failed: {}",
                rebuild_error, reindex_error
            )),
            (Err(rollback_error), _) => TalosError::StorageError(format!(
                "rebuild memory indexes after write: {}; rollback failed: {}",
                rebuild_error, rollback_error
            )),
        });
    }
    Ok(())
}

pub(super) async fn list_memory_files_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
    Query(query): Query<MemoryFilesQuery>,
) -> Result<Json<Value>, ApiError> {
    let file_list_config = &state.base_config.agent.kernel.memory;
    let limit = query
        .limit
        .unwrap_or(file_list_config.file_list_default_limit)
        .min(file_list_config.file_list_max_limit.max(1));
    let before_date = query
        .before_date
        .as_deref()
        .map(parse_memory_date)
        .transpose()?;
    let date_from = query
        .date_from
        .as_deref()
        .map(parse_memory_date)
        .transpose()?;
    let date_to = query
        .date_to
        .as_deref()
        .map(parse_memory_date)
        .transpose()?;

    let dir = memory_dir(state.as_ref());
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Json(json!({ "data": Vec::<MemoryFileSummary>::new() })));
        }
        Err(err) => {
            return Err(map_internal_error(TalosError::StorageError(
                err.to_string(),
            )))
        }
    };

    let mut files = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| map_internal_error(TalosError::StorageError(err.to_string())))?
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.ends_with(".md") {
            continue;
        }
        let date = &name[..name.len() - 3];
        if !is_memory_date(date) {
            continue;
        }
        let parsed = parse_memory_date(date)?;
        if !date_in_range(parsed, date_from, date_to, before_date) {
            continue;
        }
        let Some(detail) = load_memory_detail(state.as_ref(), date).await? else {
            continue;
        };
        let section_count =
            load_memory_section_count(state.as_ref(), date, &detail.version_token).await?;
        files.push(MemoryFileSummary {
            date: detail.date,
            path: detail.path,
            section_count,
            updated_at: detail.updated_at,
            version_token: detail.version_token,
        });
    }

    files.sort_by(|left, right| right.date.cmp(&left.date));
    files.truncate(limit);
    Ok(Json(json!({ "data": files })))
}

pub(super) async fn get_memory_file_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
    Path(date): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let Some(detail) = load_memory_detail(state.as_ref(), &date).await? else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "memory file not found",
        ));
    };
    Ok(Json(json!(detail)))
}

pub(super) async fn put_memory_file_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
    Path(date): Path<String>,
    Json(request): Json<DocUpdateRequest>,
) -> Result<Json<Value>, ApiError> {
    let path = memory_file_path(state.as_ref(), &date)?;
    with_file_lock(state.as_ref(), &path, async {
        let existing = read_optional(&path).await?;
        validate_expected_version(existing.as_deref(), &request)?;
        let rebuild_indexes = || {
            let store = MemoryStore::from_config(&state.base_config.agent.kernel);
            async move { store.rebuild_memory_indexes().await }
        };
        write_memory_file_transaction(
            &path,
            &request.content,
            existing.as_deref(),
            &rebuild_indexes,
        )
        .await?;
        Ok(())
    })
    .await
    .map_err(map_write_error)?;

    let detail = load_memory_detail(state.as_ref(), &date)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "memory file missing after write",
            )
        })?;
    Ok(Json(json!(detail)))
}

pub(super) async fn search_memory_files_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
    Query(query): Query<MemorySearchHttpQuery>,
) -> Result<Json<Value>, ApiError> {
    if query.q.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "search query cannot be empty",
        ));
    }
    let limit = query
        .limit
        .unwrap_or(state.base_config.agent.kernel.memory.search_max_results)
        .min(
            state
                .base_config
                .agent
                .kernel
                .memory
                .search_max_results
                .max(1),
        );
    let date_from = query
        .date_from
        .as_deref()
        .map(parse_memory_date)
        .transpose()?;
    let date_to = query
        .date_to
        .as_deref()
        .map(parse_memory_date)
        .transpose()?;

    let store = MemoryStore::from_config_with_llm(
        &state.base_config.agent.kernel,
        state.llm_provider.clone(),
    );
    let mut results = store
        .search_memory(&MemorySearchQuery {
            query: query.q,
            max_results: limit,
        })
        .await
        .map_err(map_internal_error)?;
    results.retain(|result| {
        let Some(file_name) = result.path.rsplit('/').next() else {
            return false;
        };
        let date = file_name.strip_suffix(".md").unwrap_or(file_name);
        let Ok(date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            return false;
        };
        date_in_range(date, date_from, date_to, None)
    });
    results.truncate(limit);
    Ok(Json(json!({ "data": results })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_in_range_applies_before_date_after_primary_range_filter() {
        let target = NaiveDate::from_ymd_opt(2026, 4, 4).expect("date should be valid");
        let date_from = Some(NaiveDate::from_ymd_opt(2026, 4, 1).expect("date should be valid"));
        let date_to = Some(NaiveDate::from_ymd_opt(2026, 4, 30).expect("date should be valid"));

        assert!(date_in_range(target, date_from, date_to, None));
        assert!(date_in_range(
            target,
            date_from,
            date_to,
            Some(NaiveDate::from_ymd_opt(2026, 4, 5).expect("date should be valid"))
        ));
        assert!(!date_in_range(
            target,
            date_from,
            date_to,
            Some(NaiveDate::from_ymd_opt(2026, 4, 4).expect("date should be valid"))
        ));
        assert!(!date_in_range(
            target,
            date_from,
            date_to,
            Some(NaiveDate::from_ymd_opt(2026, 4, 3).expect("date should be valid"))
        ));
    }

    #[tokio::test]
    async fn test_write_memory_file_transaction_removes_new_file_on_rebuild_failure() {
        let temp_dir =
            std::env::temp_dir().join(format!("talos-memory-files-test-{}", uuid::Uuid::new_v4()));
        let path = temp_dir.join("memory").join("2026-04-04.md");

        let result = write_memory_file_transaction(&path, "new content", None, &|| async {
            Err(TalosError::StorageError(
                "forced rebuild failure".to_string(),
            ))
        })
        .await;

        assert!(result.is_err());
        assert!(!path.exists(), "new file should be removed after rollback");
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn test_write_memory_file_transaction_restores_previous_content_on_rebuild_failure() {
        let temp_dir =
            std::env::temp_dir().join(format!("talos-memory-files-test-{}", uuid::Uuid::new_v4()));
        let path = temp_dir.join("memory").join("2026-04-04.md");
        write_atomic(&path, "old content")
            .await
            .expect("seed file should write");

        let result =
            write_memory_file_transaction(&path, "new content", Some("old content"), &|| async {
                Err(TalosError::StorageError(
                    "forced rebuild failure".to_string(),
                ))
            })
            .await;

        assert!(result.is_err());
        let restored = fs::read_to_string(&path)
            .await
            .expect("restored file should read");
        assert_eq!(restored, "old content");
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
