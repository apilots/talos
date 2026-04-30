use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::LlmProvider;
use talos_memory::{Memory, MemoryIndexFile, MemorySearchQuery, MemorySearchResult, MemoryStore};
use talos_types::{DocUpdateRequest, MemoryFileDetail, MemoryFileSummary, TalosConfig};
use tokio::fs;

use crate::access::{Actor, ResourceAccessService};
use crate::file::{
    content_hash, modified_at_ms, read_optional, validate_expected_version, with_file_lock,
    workspace_root, write_atomic, FileLockRegistry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryDate(NaiveDate);

impl MemoryDate {
    pub fn parse(value: &str) -> TalosResult<Self> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| TalosError::ValidationError("memory date must use YYYY-MM-DD".to_string()))
    }

    pub fn file_stem(self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryListParams {
    pub limit: Option<usize>,
    pub before_date: Option<MemoryDate>,
    pub date_from: Option<MemoryDate>,
    pub date_to: Option<MemoryDate>,
}

#[derive(Debug, Clone)]
pub struct MemorySearchParams {
    pub query: String,
    pub limit: Option<usize>,
    pub date_from: Option<MemoryDate>,
    pub date_to: Option<MemoryDate>,
}

#[derive(Clone)]
pub struct MemoryService {
    base_config: TalosConfig,
    llm_provider: Arc<dyn LlmProvider>,
    file_locks: FileLockRegistry,
}

impl MemoryService {
    pub fn new(
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        file_locks: FileLockRegistry,
    ) -> Self {
        Self {
            base_config,
            llm_provider,
            file_locks,
        }
    }

    pub async fn list_files(
        &self,
        params: MemoryListParams,
    ) -> TalosResult<Vec<MemoryFileSummary>> {
        let config = &self.base_config.agent.kernel.memory;
        let limit = params
            .limit
            .unwrap_or(config.file_list_default_limit)
            .min(config.file_list_max_limit.max(1));

        let mut entries = match tokio::fs::read_dir(self.memory_dir()).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(TalosError::StorageError(err.to_string())),
        };

        let mut files = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| TalosError::StorageError(err.to_string()))?
        {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(date_text) = name.strip_suffix(".md") else {
                continue;
            };
            let Ok(date) = MemoryDate::parse(date_text) else {
                continue;
            };
            if !date_in_range(date, params.date_from, params.date_to, params.before_date) {
                continue;
            }
            let Some(detail) = self.load_detail(date).await? else {
                continue;
            };
            files.push(MemoryFileSummary {
                date: detail.date,
                path: detail.path,
                section_count: detail.section_count,
                updated_at: detail.updated_at,
                version_token: detail.version_token,
            });
        }

        files.sort_by(|left, right| right.date.cmp(&left.date));
        files.truncate(limit);
        Ok(files)
    }

    pub async fn get_file(&self, date: MemoryDate) -> TalosResult<MemoryFileDetail> {
        self.load_detail(date)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound("memory file not found".to_string()))
    }

    pub async fn put_file(
        &self,
        actor: &Actor,
        date: MemoryDate,
        request: DocUpdateRequest,
    ) -> TalosResult<MemoryFileDetail> {
        ResourceAccessService::require_admin(actor)?;
        let path = self.memory_file_path(date);
        with_file_lock(&self.file_locks, &path, async {
            let existing = read_optional(&path).await?;
            validate_expected_version(existing.as_deref(), &request)?;
            let rebuild_indexes = || {
                let store = MemoryStore::from_config(&self.base_config.agent.kernel);
                async move { store.rebuild_memory_indexes().await }
            };
            write_memory_file_transaction(
                &path,
                &request.content,
                existing.as_deref(),
                &rebuild_indexes,
            )
            .await
        })
        .await?;

        self.load_detail(date)
            .await?
            .ok_or_else(|| TalosError::InternalError("memory file missing after write".to_string()))
    }

    pub async fn search_files(
        &self,
        params: MemorySearchParams,
    ) -> TalosResult<Vec<MemorySearchResult>> {
        if params.query.trim().is_empty() {
            return Err(TalosError::ValidationError(
                "search query cannot be empty".to_string(),
            ));
        }
        let max_results = self
            .base_config
            .agent
            .kernel
            .memory
            .search_max_results
            .max(1);
        let limit = params.limit.unwrap_or(max_results).min(max_results);
        let store = MemoryStore::from_config_with_llm(
            &self.base_config.agent.kernel,
            self.llm_provider.clone(),
        );
        let mut results = store
            .search_memory(&MemorySearchQuery {
                query: params.query,
                max_results: limit,
            })
            .await?;
        results.retain(|result| {
            let Some(file_name) = result.path.rsplit('/').next() else {
                return false;
            };
            let date_text = file_name.strip_suffix(".md").unwrap_or(file_name);
            let Ok(date) = MemoryDate::parse(date_text) else {
                return false;
            };
            date_in_range(date, params.date_from, params.date_to, None)
        });
        results.truncate(limit);
        Ok(results)
    }

    fn memory_dir(&self) -> PathBuf {
        workspace_root(&self.base_config).join(&self.base_config.agent.kernel.memory.memory_dirname)
    }

    fn memory_file_path(&self, date: MemoryDate) -> PathBuf {
        self.memory_dir().join(format!("{}.md", date.file_stem()))
    }

    fn memory_index_path(&self, date: MemoryDate) -> PathBuf {
        self.memory_file_path(date).with_extension("index.json")
    }

    async fn load_detail(&self, date: MemoryDate) -> TalosResult<Option<MemoryFileDetail>> {
        let path = self.memory_file_path(date);
        let content = read_optional(&path).await?;
        let updated_at = modified_at_ms(&path).await?;
        match content {
            Some(content) => {
                let version_token = content_hash(&content);
                let section_count = self.load_section_count(date, &version_token).await?;
                let date_text = date.file_stem();
                Ok(Some(MemoryFileDetail {
                    date: date_text.clone(),
                    path: format!("memory/{date_text}.md"),
                    section_count,
                    version_token,
                    updated_at,
                    content,
                }))
            }
            None => Ok(None),
        }
    }

    async fn load_section_count(
        &self,
        date: MemoryDate,
        version_token: &str,
    ) -> TalosResult<usize> {
        let index_path = self.memory_index_path(date);
        let Some(index_content) = read_optional(&index_path).await? else {
            return Ok(0);
        };
        let parsed = serde_json::from_str::<MemoryIndexFile>(&index_content).map_err(|err| {
            TalosError::StorageError(format!(
                "parse memory index {}: {}",
                index_path.display(),
                err
            ))
        })?;
        if parsed.source_content_hash != version_token {
            return Ok(0);
        }
        Ok(parsed.sections.len())
    }
}

fn date_in_range(
    date: MemoryDate,
    date_from: Option<MemoryDate>,
    date_to: Option<MemoryDate>,
    before_date: Option<MemoryDate>,
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

async fn restore_previous_memory_file(
    path: &Path,
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
    path: &Path,
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

#[cfg(test)]
mod tests {
    use super::{date_in_range, write_memory_file_transaction, MemoryDate};
    use crate::file::write_atomic;
    use talos_error::TalosError;

    #[test]
    fn date_range_applies_before_date_after_primary_range_filter() {
        let target = MemoryDate::parse("2026-04-04").expect("date should be valid");
        let date_from = Some(MemoryDate::parse("2026-04-01").expect("date should be valid"));
        let date_to = Some(MemoryDate::parse("2026-04-30").expect("date should be valid"));

        assert!(date_in_range(target, date_from, date_to, None));
        assert!(date_in_range(
            target,
            date_from,
            date_to,
            Some(MemoryDate::parse("2026-04-05").expect("date should be valid"))
        ));
        assert!(!date_in_range(
            target,
            date_from,
            date_to,
            Some(MemoryDate::parse("2026-04-04").expect("date should be valid"))
        ));
        assert!(!date_in_range(
            target,
            date_from,
            date_to,
            Some(MemoryDate::parse("2026-04-03").expect("date should be valid"))
        ));
    }

    #[tokio::test]
    async fn write_transaction_removes_new_file_on_rebuild_failure() {
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
    async fn write_transaction_restores_previous_content_on_rebuild_failure() {
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
        let restored = tokio::fs::read_to_string(&path)
            .await
            .expect("restored file should read");
        assert_eq!(restored, "old content");
        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
