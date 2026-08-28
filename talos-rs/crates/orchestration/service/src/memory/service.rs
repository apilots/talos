use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use talos_config::{parse_timezone, AgentKernelConfig, AgentRuntimeConfig, TalosTimezone};
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDayRevision, DreamMemoryDaySnapshot, DreamMemoryDocument,
    DreamMemoryDocumentContext, DreamMemoryFormationStore, DreamMemoryWriteRequest, MemoryAgentId,
    MemoryReader, MemoryScope, MemoryTimezone, MemoryUserId,
};
use talos_store::memory::{
    DreamMemorySourceReplace, MemoryRecord, MemorySearchQuery, MemorySearchRecord, MemoryStore,
};
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, MemoryFileDetail, MemoryFileListRequest,
    MemoryFileListResponse, MemoryFileReadRequest, MemoryFileSummary, MemoryFileWriteRequest,
    MemoryGetRequest, MemoryGetResponse, MemoryInfo, MemorySearchHit, MemorySearchRequest,
    MemorySearchResponse, MemorySourceInfo, TalosConfig,
};

use super::legacy_converter::convert_legacy_scope;
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
    agent_workspaces: Arc<Vec<AgentMemoryWorkspace>>,
    memory_dirname: String,
}

#[derive(Debug, Clone)]
struct AgentMemoryWorkspace {
    agent_id: String,
    users_root: PathBuf,
    timezone: TalosTimezone,
}

impl MemoryService {
    pub fn new(base_config: TalosConfig, store: Arc<MemoryStore>) -> TalosResult<Self> {
        let resolver = EffectiveWorkspaceResolver::new(base_config.clone());
        let agent_workspaces = resolve_agent_memory_workspaces(&base_config)?;
        let memory_dirname = base_config.memory.memory_dirname.clone();
        Ok(Self {
            resolver,
            store,
            agent_workspaces: Arc::new(agent_workspaces),
            memory_dirname,
        })
    }

    /// Runs the release-scoped legacy conversion before normal Dream Memory startup rebuild.
    ///
    /// TODO(dream-memory-okf): Remove this one-time legacy converter in the next release.
    pub async fn convert_legacy_known_scopes(&self) -> TalosResult<()> {
        for agent_workspace in self.agent_workspaces.iter() {
            for scope in known_user_scopes(agent_workspace)? {
                let Some(conversion) = convert_legacy_scope(
                    &scope.agent_id,
                    &scope.user_id,
                    &scope.workspace_root,
                    &self.memory_dirname,
                    agent_workspace.timezone,
                )
                .await?
                else {
                    continue;
                };
                self.store
                    .rebuild_scope(&scope, &self.memory_dirname, agent_workspace.timezone)
                    .await?;
                conversion.finish().await?;
            }
        }
        Ok(())
    }

    pub async fn rebuild_known_scopes(&self) -> TalosResult<()> {
        for agent_workspace in self.agent_workspaces.iter() {
            for scope in known_user_scopes(agent_workspace)? {
                self.store
                    .rebuild_scope(&scope, &self.memory_dirname, agent_workspace.timezone)
                    .await?;
            }
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
        let scope = memory_scope(&workspace);
        let timezone = self.timezone_for_scope(&scope)?;
        let mut dates = Vec::new();
        for indexed_date in self
            .store
            .list_scope_dates(&scope, &self.memory_dirname, timezone)
            .await?
        {
            let date = MemoryDate::parse(&indexed_date.to_string())?;
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
                .load_detail(
                    &workspace.config,
                    &workspace.agent_id,
                    &workspace.user_id,
                    date,
                )
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
        self.load_detail(
            &workspace.config,
            &workspace.agent_id,
            &workspace.user_id,
            date,
        )
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
        let source_date = DreamMemoryDate::parse(&date.file_stem()).map_err(memory_domain_error)?;
        let scope = memory_scope(&workspace);
        let memory_dirname = workspace.config.memory.memory_dirname.clone();
        let timezone = parse_timezone(&workspace.config.agent.kernel.timezone)?;
        let existing = talos_fs::read_file(&path).await?;
        if existing.as_deref() != Some(request.content.as_str()) {
            ensure_workspace_write_version(existing.as_deref(), &request)?;
        }
        self.store
            .replace_source(DreamMemorySourceReplace {
                scope: &scope,
                memory_dirname: &memory_dirname,
                source_date,
                expected_content: existing.as_deref(),
                updated: &request.content,
                timezone,
            })
            .await?;

        self.load_detail(
            &workspace.config,
            &workspace.agent_id,
            &workspace.user_id,
            date,
        )
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
        agent_id: &str,
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
                let source_date =
                    DreamMemoryDate::parse(&date.file_stem()).map_err(memory_domain_error)?;
                Ok(MemoryFileDetail {
                    date: date.file_stem(),
                    section_count: self.store.source_section_count(
                        agent_id,
                        user_id,
                        source_date,
                        &content,
                        parse_timezone(&config.agent.kernel.timezone)?,
                    )?,
                    version_token: workspace_revision(&content),
                    updated_at,
                    content,
                })
            })
            .transpose()
    }

    fn timezone_for_scope(&self, scope: &MemoryScope) -> TalosResult<TalosTimezone> {
        self.agent_workspaces
            .iter()
            .find(|workspace| workspace.agent_id == scope.agent_id)
            .map(|workspace| workspace.timezone)
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("unknown memory agent_id: {}", scope.agent_id),
                )
            })
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
            let timezone = self.timezone_for_scope(scope)?;
            let records = self
                .store
                .search_scope(scope, &self.memory_dirname, timezone, &query)
                .await?;
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
            let timezone = self.timezone_for_scope(scope)?;
            let record = self
                .store
                .get_scope(scope, &self.memory_dirname, timezone, memory_id)
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

impl DreamMemoryFormationStore for MemoryService {
    fn read_day<'a>(
        &'a self,
        scope: &'a MemoryScope,
        date: DreamMemoryDate,
    ) -> Pin<Box<dyn Future<Output = TalosResult<DreamMemoryDaySnapshot>> + Send + 'a>> {
        Box::pin(async move {
            let timezone = self.timezone_for_scope(scope)?;
            let path = scope
                .workspace_root
                .join(&self.memory_dirname)
                .join(format!("{date}.md"));
            let content = talos_fs::read_file(&path).await?;
            let entries = match content.as_deref() {
                Some(markdown) => {
                    let context = DreamMemoryDocumentContext::new(
                        MemoryAgentId::parse(scope.agent_id.clone())
                            .map_err(memory_domain_error)?,
                        MemoryUserId::parse(scope.user_id.clone()).map_err(memory_domain_error)?,
                        date,
                        MemoryTimezone::new(timezone),
                    );
                    DreamMemoryDocument::parse(context, markdown)
                        .map_err(memory_domain_error)?
                        .entries()
                        .to_vec()
                }
                None => Vec::new(),
            };
            Ok(DreamMemoryDaySnapshot::new(
                date,
                DreamMemoryDayRevision::from_content(content.as_deref()),
                entries,
            ))
        })
    }

    fn write<'a>(
        &'a self,
        scope: &'a MemoryScope,
        request: DreamMemoryWriteRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let timezone = self.timezone_for_scope(scope)?;
            self.store
                .write_batch(scope, &self.memory_dirname, timezone, &request)
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
        memory_id: record.memory.memory_id.as_str().to_string(),
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
        memory_id: record.memory_id.as_str().to_string(),
        title: record.title,
        body: record.body,
        kind: record.kind,
        importance: record.importance,
        occurred_at: record.occurred_at,
        source,
    }
}

fn memory_source(record: &MemoryRecord) -> MemorySourceInfo {
    match &record.provenance {
        talos_memory::DreamMemoryProvenance::Generated {
            session_id,
            source_range,
        } => MemorySourceInfo {
            date: record.source_date.to_string(),
            session_id: Some(session_id.as_str().to_string()),
            from_seq: Some(source_range.from_seq()),
            until_seq: Some(source_range.until_seq()),
        },
        talos_memory::DreamMemoryProvenance::UserAuthored => MemorySourceInfo {
            date: record.source_date.to_string(),
            session_id: None,
            from_seq: None,
            until_seq: None,
        },
    }
}

fn memory_scope(workspace: &crate::workspace::EffectiveAgentWorkspace) -> MemoryScope {
    MemoryScope {
        agent_id: workspace.agent_id.clone(),
        user_id: workspace.user_id.clone(),
        workspace_root: workspace.user_workspace_dir.clone(),
    }
}

fn resolve_agent_memory_workspaces(config: &TalosConfig) -> TalosResult<Vec<AgentMemoryWorkspace>> {
    if config.agent.agents.is_empty() {
        return Ok(vec![agent_memory_workspace(
            config,
            "default",
            &config.agent.kernel,
        )?]);
    }
    config
        .agent
        .agents
        .iter()
        .filter(|definition| definition.enabled)
        .map(|definition| {
            let kernel = match &definition.runtime_config {
                Some(AgentRuntimeConfig::Kernel { kernel }) => kernel,
                None => &config.agent.kernel,
            };
            agent_memory_workspace(config, definition.id.trim(), kernel)
        })
        .collect()
}

fn agent_memory_workspace(
    config: &TalosConfig,
    agent_id: &str,
    kernel: &AgentKernelConfig,
) -> TalosResult<AgentMemoryWorkspace> {
    let mut scoped_config = config.clone();
    scoped_config.agent.kernel = kernel.clone();
    Ok(AgentMemoryWorkspace {
        agent_id: agent_id.to_string(),
        users_root: resolve_workspace_root(&scoped_config).join("users"),
        timezone: parse_timezone(&kernel.timezone)?,
    })
}

fn known_user_scopes(agent_workspace: &AgentMemoryWorkspace) -> TalosResult<Vec<MemoryScope>> {
    let Some(entries) = talos_fs::read_directory(&agent_workspace.users_root)? else {
        return Ok(Vec::new());
    };
    Ok(entries
        .into_iter()
        .filter(|entry| entry.metadata.is_dir && !entry.metadata.is_symlink)
        .filter_map(|entry| {
            let user_id = entry.path.file_name()?.to_str()?.to_string();
            Some(MemoryScope {
                agent_id: agent_workspace.agent_id.clone(),
                user_id,
                workspace_root: entry.path,
            })
        })
        .collect())
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

fn memory_domain_error(error: talos_memory::DreamMemoryError) -> TalosError {
    TalosError::public(talos_error::ErrorCode::InvalidRequest, error.to_string())
}

fn parse_optional_memory_date(value: Option<&str>) -> TalosResult<Option<MemoryDate>> {
    value.map(MemoryDate::parse).transpose()
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
