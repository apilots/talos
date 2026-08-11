use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use talos_config::parse_timezone;
use talos_error::{TalosError, TalosResult};
use talos_memory::{MemoryReader, MemoryScope, MemoryWriteBatch, MemoryWriter};
use talos_store::memory::{MemoryRecord, MemorySearchQuery, MemorySearchRecord, MemoryStore};
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, MemoryFileDetail, MemoryFileListRequest,
    MemoryFileListResponse, MemoryFileReadRequest, MemoryFileSummary, MemoryFileWriteRequest,
    MemoryGetRequest, MemoryGetResponse, MemoryInfo, MemorySearchHit, MemorySearchRequest,
    MemorySearchResponse, MemorySourceInfo, TalosConfig,
};

use super::types::MemoryDate;
use crate::access::Actor;
use crate::workspace::file::{
    ensure_workspace_write_version, resolve_workspace_root, workspace_revision,
};
use crate::workspace::EffectiveWorkspaceResolver;

const DEFAULT_SEARCH_LIMIT: u32 = 6;
const MAX_SEARCH_LIMIT: u32 = 20;
const MAX_QUERY_CHARS: usize = 512;

/// Actor-scoped application boundary for memory files and long-term memory records.
#[derive(Clone)]
pub struct MemoryService {
    resolver: EffectiveWorkspaceResolver,
    store: Arc<MemoryStore>,
    timezone: talos_config::TalosTimezone,
    users_root: PathBuf,
    memory_dirname: String,
}

impl MemoryService {
    pub fn new(base_config: TalosConfig, store: Arc<MemoryStore>) -> TalosResult<Self> {
        let timezone = parse_timezone(&base_config.agent.kernel.timezone)?;
        let users_root = resolve_workspace_root(&base_config).join("users");
        let memory_dirname = base_config.memory.memory_dirname.clone();
        Ok(Self {
            resolver: EffectiveWorkspaceResolver::new(base_config),
            store,
            timezone,
            users_root,
            memory_dirname,
        })
    }

    pub async fn rebuild_known_scopes(&self) -> TalosResult<()> {
        let Some(entries) = talos_fs::read_directory(&self.users_root)? else {
            return Ok(());
        };
        for entry in entries {
            if !entry.metadata.is_dir || entry.metadata.is_symlink {
                continue;
            }
            let Some(user_id) = entry.path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            self.store
                .rebuild_scope(user_id, &entry.path, &self.memory_dirname, self.timezone)
                .await?;
        }
        Ok(())
    }

    pub async fn list_files_with_request(
        &self,
        actor: &Actor,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        let config = &workspace.config.memory;
        let limit = request
            .limit
            .unwrap_or(config.file_list_default_limit)
            .min(config.file_list_max_limit.max(1));
        let before_date = parse_optional_memory_date(request.before_date.as_deref())?;
        let date_from = parse_optional_memory_date(request.date_from.as_deref())?;
        let date_to = parse_optional_memory_date(request.date_to.as_deref())?;
        let Some(entries) = talos_fs::read_directory(&self.memory_dir(&workspace.config))? else {
            return Ok(MemoryFileListResponse { data: Vec::new() });
        };

        let mut dates = Vec::new();
        for entry in entries {
            let Some(name) = entry.path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(date_text) = name.strip_suffix(".md") else {
                continue;
            };
            let Ok(date) = MemoryDate::parse(date_text) else {
                continue;
            };
            if !date_in_range(date, date_from, date_to, before_date) {
                continue;
            }
            dates.push(date);
        }
        dates.sort_by(|left, right| right.cmp(left));
        dates.truncate(limit);

        let mut files = Vec::with_capacity(dates.len());
        for date in dates {
            let Some(detail) = self
                .load_detail(&workspace.config, &workspace.user_id, date)
                .await?
            else {
                continue;
            };
            files.push(MemoryFileSummary {
                date: detail.date,
                section_count: detail.section_count,
                updated_at: detail.updated_at,
                version_token: detail.version_token,
            });
        }
        Ok(MemoryFileListResponse { data: files })
    }

    async fn get_file(&self, actor: &Actor, date: MemoryDate) -> TalosResult<MemoryFileDetail> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        self.load_detail(&workspace.config, &workspace.user_id, date)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    "memory file not found".to_string(),
                )
            })
    }

    pub async fn read_file_with_request(
        &self,
        actor: &Actor,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileDetail> {
        self.get_file(actor, MemoryDate::parse(&request.date)?)
            .await
    }

    async fn put_file(
        &self,
        actor: &Actor,
        date: MemoryDate,
        request: DocUpdateRequest,
    ) -> TalosResult<MemoryFileDetail> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        self.resolver.write_profile_snapshot(&workspace).await?;
        let path = self.memory_file_path(&workspace.config, date);
        let source_path = format!(
            "{}/{}.md",
            workspace.config.memory.memory_dirname,
            date.file_stem()
        );
        talos_fs::with_file_lock(&path, || async {
            let existing = talos_fs::read_file(&path).await?;
            if existing.as_deref() == Some(request.content.as_str()) {
                return self
                    .store
                    .sync_source(
                        &workspace.user_id,
                        &source_path,
                        &request.content,
                        self.timezone,
                    )
                    .await;
            }
            ensure_workspace_write_version(existing.as_deref(), &request)?;
            self.store
                .validate_source_edit(
                    &workspace.user_id,
                    &source_path,
                    existing.as_deref(),
                    &request.content,
                    self.timezone,
                )
                .await?;
            talos_fs::write_file(&path, &request.content).await?;
            self.store
                .sync_source(
                    &workspace.user_id,
                    &source_path,
                    &request.content,
                    self.timezone,
                )
                .await
        })
        .await?;

        self.load_detail(&workspace.config, &workspace.user_id, date)
            .await?
            .ok_or_else(|| TalosError::internal("memory file missing after write".to_string()))
    }

    pub async fn write_file_with_request(
        &self,
        actor: &Actor,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileDetail> {
        let date = MemoryDate::parse(&request.date)?;
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

    pub async fn search(
        &self,
        actor: &Actor,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        MemoryReader::search(self, &memory_scope(&workspace), request).await
    }

    pub async fn get(
        &self,
        actor: &Actor,
        request: MemoryGetRequest,
    ) -> TalosResult<MemoryGetResponse> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        MemoryReader::get(self, &memory_scope(&workspace), request).await
    }

    pub(crate) async fn recent_high(&self, user_id: &str) -> TalosResult<Vec<MemoryInfo>> {
        let records = self.store.list_recent_high(user_id, 20).await?;
        Ok(records.into_iter().map(memory_info).collect())
    }

    fn memory_dir(&self, config: &TalosConfig) -> PathBuf {
        resolve_workspace_root(config).join(&config.memory.memory_dirname)
    }

    fn memory_file_path(&self, config: &TalosConfig, date: MemoryDate) -> PathBuf {
        self.memory_dir(config)
            .join(format!("{}.md", date.file_stem()))
    }

    async fn load_detail(
        &self,
        config: &TalosConfig,
        user_id: &str,
        date: MemoryDate,
    ) -> TalosResult<Option<MemoryFileDetail>> {
        let path = self.memory_file_path(config, date);
        let content = talos_fs::read_file(&path).await?;
        let updated_at = talos_fs::get_file_metadata(&path)
            .await?
            .and_then(|metadata| metadata.modified_at_ms);
        content
            .map(|content| {
                let source_path =
                    format!("{}/{}.md", config.memory.memory_dirname, date.file_stem());
                Ok(MemoryFileDetail {
                    date: date.file_stem(),
                    section_count: self.store.source_section_count(
                        user_id,
                        &source_path,
                        &content,
                        self.timezone,
                    )?,
                    version_token: workspace_revision(&content),
                    updated_at,
                    content,
                })
            })
            .transpose()
    }
}

impl MemoryReader for MemoryService {
    fn search<'a>(
        &'a self,
        scope: &'a MemoryScope,
        request: MemorySearchRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemorySearchResponse>> + Send + 'a>> {
        Box::pin(async move {
            let query = validate_search_request(request)?;
            let records = self.store.search(&scope.user_id, &query).await?;
            Ok(MemorySearchResponse {
                data: records.into_iter().map(memory_search_hit).collect(),
            })
        })
    }

    fn get<'a>(
        &'a self,
        scope: &'a MemoryScope,
        request: MemoryGetRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemoryGetResponse>> + Send + 'a>> {
        Box::pin(async move {
            let memory_id = request.memory_id.trim();
            if memory_id.is_empty() {
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    "memory_id cannot be empty".to_string(),
                ));
            }
            let record = self
                .store
                .get(&scope.user_id, memory_id)
                .await?
                .ok_or_else(|| {
                    TalosError::public(
                        talos_error::ErrorCode::ResourceNotFound,
                        "memory not found".to_string(),
                    )
                })?;
            Ok(MemoryGetResponse {
                memory: memory_info(record),
            })
        })
    }
}

impl MemoryWriter for MemoryService {
    fn write<'a>(
        &'a self,
        scope: &'a MemoryScope,
        batch: MemoryWriteBatch,
    ) -> Pin<Box<dyn Future<Output = TalosResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.store
                .write_batch(
                    &scope.user_id,
                    &scope.workspace_root,
                    &self.memory_dirname,
                    self.timezone,
                    &batch,
                )
                .await
        })
    }
}

fn validate_search_request(request: MemorySearchRequest) -> TalosResult<MemorySearchQuery> {
    let text = request.query.trim().to_string();
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory query must be non-empty text".to_string(),
        ));
    }
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("memory query exceeds {MAX_QUERY_CHARS} characters"),
        ));
    }
    let limit = request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("memory search limit must be between 1 and {MAX_SEARCH_LIMIT}"),
        ));
    }
    if request.kinds.as_ref().is_some_and(Vec::is_empty) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory kinds cannot be empty".to_string(),
        ));
    }
    if matches!((request.since_at, request.until_at), (Some(since), Some(until)) if since > until) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory since_at must not exceed until_at".to_string(),
        ));
    }
    Ok(MemorySearchQuery {
        text,
        limit,
        kinds: request.kinds,
        since_at: request.since_at,
        until_at: request.until_at,
    })
}

fn memory_search_hit(record: MemorySearchRecord) -> MemorySearchHit {
    let source = memory_source(&record.memory);
    MemorySearchHit {
        memory_id: record.memory.memory_id,
        title: record.memory.title,
        preview: record.preview,
        kind: record.memory.kind,
        importance: record.memory.importance,
        occurred_at: record.memory.occurred_at,
        rank: record.rank,
        source,
    }
}

fn memory_info(record: MemoryRecord) -> MemoryInfo {
    let source = memory_source(&record);
    MemoryInfo {
        memory_id: record.memory_id,
        title: record.title,
        body: record.body,
        kind: record.kind,
        importance: record.importance,
        occurred_at: record.occurred_at,
        source,
    }
}

fn memory_source(record: &MemoryRecord) -> MemorySourceInfo {
    MemorySourceInfo {
        date: record.source_date.clone(),
        session_id: record.session_id.clone(),
        from_seq: record.source_from_seq,
        until_seq: record.source_until_seq,
    }
}

fn memory_scope(workspace: &crate::workspace::EffectiveAgentWorkspace) -> MemoryScope {
    MemoryScope {
        user_id: workspace.user_id.clone(),
        workspace_root: workspace.user_workspace_dir.clone(),
    }
}

fn date_in_range(
    date: MemoryDate,
    date_from: Option<MemoryDate>,
    date_to: Option<MemoryDate>,
    before_date: Option<MemoryDate>,
) -> bool {
    date_from.is_none_or(|from| date >= from)
        && date_to.is_none_or(|to| date <= to)
        && before_date.is_none_or(|before| date < before)
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

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
