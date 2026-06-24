use std::path::{Path, PathBuf};
use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_llm_provider::LlmProvider;
use talos_memory::{Memory, MemoryIndexFile, MemorySearchQuery, MemorySearchResult, MemoryStore};
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, MemoryFileDetail, MemoryFileListRequest,
    MemoryFileListResponse, MemoryFileReadRequest, MemoryFileSummary, MemoryFileWriteRequest,
    MemorySearchRequest, MemorySearchResponse, TalosConfig,
};

use super::types::{MemoryDate, MemoryListCommand, MemorySearchCommand};
use crate::access::Actor;
use crate::workspace::file::{
    ensure_workspace_write_version, resolve_workspace_root, workspace_revision,
};
use crate::workspace::EffectiveWorkspaceResolver;

#[derive(Clone)]
pub struct MemoryService {
    resolver: EffectiveWorkspaceResolver,
    llm_provider: Arc<dyn LlmProvider>,
}

impl MemoryService {
    pub fn new(base_config: TalosConfig, llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            resolver: EffectiveWorkspaceResolver::new(base_config.clone()),
            llm_provider,
        }
    }

    pub async fn list_files(
        &self,
        actor: &Actor,
        params: MemoryListCommand,
    ) -> TalosResult<Vec<MemoryFileSummary>> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        let config = &workspace.config.agent.kernel.memory;
        let limit = params
            .limit
            .unwrap_or(config.file_list_default_limit)
            .min(config.file_list_max_limit.max(1));

        let entries = match talos_fs::read_directory(&self.memory_dir(&workspace.config))? {
            Some(entries) => entries,
            None => return Ok(Vec::new()),
        };

        let mut files = Vec::new();
        for entry in entries {
            let path = entry.path;
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
            let Some(detail) = self.load_detail(&workspace.config, date).await? else {
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

    pub async fn list_files_with_request(
        &self,
        actor: &Actor,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        let files = self
            .list_files(
                actor,
                MemoryListCommand {
                    limit: request.limit,
                    before_date: parse_optional_memory_date(request.before_date.as_deref())?,
                    date_from: parse_optional_memory_date(request.date_from.as_deref())?,
                    date_to: parse_optional_memory_date(request.date_to.as_deref())?,
                },
            )
            .await?;
        Ok(MemoryFileListResponse { data: files })
    }

    pub async fn get_file(&self, actor: &Actor, date: MemoryDate) -> TalosResult<MemoryFileDetail> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        self.load_detail(&workspace.config, date)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound("memory file not found".to_string()))
    }

    pub async fn read_file_with_request(
        &self,
        actor: &Actor,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileDetail> {
        self.get_file(actor, memory_date_from_path(&request.path)?)
            .await
    }

    pub async fn put_file(
        &self,
        actor: &Actor,
        date: MemoryDate,
        request: DocUpdateRequest,
    ) -> TalosResult<MemoryFileDetail> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        self.resolver.write_profile_snapshot(&workspace).await?;
        let path = self.memory_file_path(&workspace.config, date);
        talos_fs::with_file_lock(&path, || async {
            let existing = talos_fs::read_file(&path).await?;
            ensure_workspace_write_version(existing.as_deref(), &request)?;
            let rebuild_indexes = || {
                let store = MemoryStore::from_config(&workspace.config.agent.kernel);
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

        self.load_detail(&workspace.config, date)
            .await?
            .ok_or_else(|| TalosError::InternalError("memory file missing after write".to_string()))
    }

    pub async fn write_file_with_request(
        &self,
        actor: &Actor,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileDetail> {
        let date = memory_date_from_path(&request.path)?;
        self.put_file(
            actor,
            date,
            DocUpdateRequest {
                content: request.content,
                base_version: expected_version(request.expected_version),
            },
        )
        .await
    }

    pub async fn search_files(
        &self,
        actor: &Actor,
        params: MemorySearchCommand,
    ) -> TalosResult<Vec<MemorySearchResult>> {
        if params.query.trim().is_empty() {
            return Err(TalosError::ValidationError(
                "search query cannot be empty".to_string(),
            ));
        }
        let workspace = self.resolver.resolve_for_actor(actor)?;
        let max_results = workspace
            .config
            .agent
            .kernel
            .memory
            .search_max_results
            .max(1);
        let limit = params.limit.unwrap_or(max_results).min(max_results);
        let store = MemoryStore::from_config_with_llm(
            &workspace.config.agent.kernel,
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

    pub async fn search_files_with_request(
        &self,
        actor: &Actor,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        let results = self
            .search_files(
                actor,
                MemorySearchCommand {
                    query: request.query,
                    limit: request.limit,
                    date_from: parse_optional_memory_date(request.date_from.as_deref())?,
                    date_to: parse_optional_memory_date(request.date_to.as_deref())?,
                },
            )
            .await?;
        Ok(MemorySearchResponse { results })
    }

    fn memory_dir(&self, config: &TalosConfig) -> PathBuf {
        resolve_workspace_root(config).join(&config.agent.kernel.memory.memory_dirname)
    }

    fn memory_file_path(&self, config: &TalosConfig, date: MemoryDate) -> PathBuf {
        self.memory_dir(config)
            .join(format!("{}.md", date.file_stem()))
    }

    fn memory_index_path(&self, config: &TalosConfig, date: MemoryDate) -> PathBuf {
        self.memory_file_path(config, date)
            .with_extension("index.json")
    }

    async fn load_detail(
        &self,
        config: &TalosConfig,
        date: MemoryDate,
    ) -> TalosResult<Option<MemoryFileDetail>> {
        let path = self.memory_file_path(config, date);
        let content = talos_fs::read_file(&path).await?;
        let updated_at = talos_fs::get_file_metadata(&path)
            .await?
            .and_then(|metadata| metadata.modified_at_ms);
        match content {
            Some(content) => {
                let version_token = workspace_revision(&content);
                let section_count = self
                    .load_section_count(config, date, &version_token)
                    .await?;
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
        config: &TalosConfig,
        date: MemoryDate,
        version_token: &str,
    ) -> TalosResult<usize> {
        let index_path = self.memory_index_path(config, date);
        let Some(index_content) = talos_fs::read_file(&index_path).await? else {
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

fn memory_date_from_path(path: &Path) -> TalosResult<MemoryDate> {
    let stem = path
        .file_stem()
        .or_else(|| path.file_name())
        .and_then(|value| value.to_str())
        .ok_or_else(|| TalosError::ValidationError("memory file path is invalid".to_string()))?;
    MemoryDate::parse(stem)
}

fn expected_version(value: Option<String>) -> ExpectedFileVersion {
    match value {
        Some(version_token) => ExpectedFileVersion::Match { version_token },
        None => ExpectedFileVersion::MustNotExist,
    }
}

fn parse_optional_memory_date(value: Option<&str>) -> TalosResult<Option<MemoryDate>> {
    value.map(MemoryDate::parse).transpose()
}

async fn restore_previous_memory_file(
    path: &Path,
    previous_content: Option<&str>,
) -> TalosResult<()> {
    match previous_content {
        Some(content) => talos_fs::write_file(path, content).await,
        None => talos_fs::remove_file(path),
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
    talos_fs::write_file(path, content).await?;
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
        let _ = talos_fs::remove_directory(&temp_dir).await;
    }

    #[tokio::test]
    async fn write_transaction_restores_previous_content_on_rebuild_failure() {
        let temp_dir =
            std::env::temp_dir().join(format!("talos-memory-files-test-{}", uuid::Uuid::new_v4()));
        let path = temp_dir.join("memory").join("2026-04-04.md");
        talos_fs::write_file(&path, "old content")
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
        let restored = talos_fs::read_file(&path)
            .await
            .expect("restored file should read")
            .expect("restored file should exist");
        assert_eq!(restored, "old content");
        let _ = talos_fs::remove_directory(&temp_dir).await;
    }
}
