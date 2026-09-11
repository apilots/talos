use std::path::PathBuf;
use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_memory::MemoryScope;
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, TalosConfig, WorkspaceDocInfo, WorkspaceDocKind,
    WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocSummary, WorkspaceDocWriteRequest,
};

use crate::access::Actor;
use crate::workspace::file::{
    ensure_workspace_write_version, resolve_workspace_root, workspace_revision,
};
use crate::workspace::EffectiveWorkspaceResolver;
use crate::UserWorkspaceMemoryService;

#[derive(Clone)]
pub struct WorkspaceDocService {
    resolver: EffectiveWorkspaceResolver,
    memory: Arc<UserWorkspaceMemoryService>,
}

impl WorkspaceDocService {
    pub fn new(base_config: TalosConfig) -> Self {
        Self {
            resolver: EffectiveWorkspaceResolver::new(base_config),
            memory: Arc::new(UserWorkspaceMemoryService::new()),
        }
    }

    /// Reuses the application-owned User Workspace Memory service.
    #[must_use]
    pub fn with_user_workspace_memory(mut self, memory: Arc<UserWorkspaceMemoryService>) -> Self {
        self.memory = memory;
        self
    }

    /// Inspect the actor's default workspace without creating directories or files.
    pub fn workspace_info(
        &self,
        actor: &Actor,
        _request: talos_types::WorkspaceInfoRequest,
    ) -> TalosResult<talos_types::WorkspaceInfoResponse> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        Ok(talos_types::WorkspaceInfoResponse {
            workspace_id: workspace.workspace_id,
            root_prefix: workspace.base_workspace_dir.to_string_lossy().into_owned(),
            user_workspace_dir: workspace.user_workspace_dir.to_string_lossy().into_owned(),
        })
    }

    pub fn parse_kind(raw: &str) -> TalosResult<WorkspaceDocKind> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "soul" => Ok(WorkspaceDocKind::Soul),
            "identity" => Ok(WorkspaceDocKind::Identity),
            "user" => Ok(WorkspaceDocKind::User),
            "heartbeat" => Ok(WorkspaceDocKind::Heartbeat),
            "memory" => Ok(WorkspaceDocKind::Memory),
            _ => Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "unsupported workspace doc kind".to_string(),
            )),
        }
    }

    pub async fn list_docs(&self, actor: &Actor) -> TalosResult<Vec<WorkspaceDocSummary>> {
        let mut docs = Vec::new();
        for kind in supported_doc_kinds() {
            let detail = self.get_doc(actor, kind).await?;
            docs.push(WorkspaceDocSummary {
                kind,
                path: detail.path,
                exists: detail.exists,
                updated_at: detail.updated_at,
                version_token: detail.version_token,
            });
        }
        Ok(docs)
    }

    pub async fn list_docs_with_request(
        &self,
        actor: &Actor,
        _request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        let docs = self.list_docs(actor).await?;
        Ok(WorkspaceDocListResponse { data: docs })
    }

    pub async fn get_doc(
        &self,
        actor: &Actor,
        kind: WorkspaceDocKind,
    ) -> TalosResult<WorkspaceDocInfo> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        if kind == WorkspaceDocKind::Memory {
            let snapshot = self.memory.read(&memory_scope(&workspace)).await?;
            return Ok(WorkspaceDocInfo {
                kind,
                path: kind.file_name().to_string(),
                exists: snapshot.exists(),
                updated_at: snapshot.updated_at(),
                version_token: snapshot.version_token().map(str::to_string),
                content: snapshot.raw_content().to_string(),
            });
        }
        let path = self.doc_path(&workspace.config, kind);
        let content = talos_fs::read_file(&path).await?;
        let updated_at = talos_fs::get_file_metadata(&path)
            .await?
            .and_then(|metadata| metadata.modified_at_ms);
        match content {
            Some(content) => Ok(WorkspaceDocInfo {
                kind,
                path: kind.file_name().to_string(),
                exists: true,
                updated_at,
                version_token: Some(workspace_revision(&content)),
                content,
            }),
            None => Ok(WorkspaceDocInfo {
                kind,
                path: kind.file_name().to_string(),
                exists: false,
                updated_at: None,
                version_token: None,
                content: String::new(),
            }),
        }
    }

    pub async fn read_doc_with_request(
        &self,
        actor: &Actor,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocInfo> {
        self.get_doc(actor, request.kind).await
    }

    pub async fn put_doc(
        &self,
        actor: &Actor,
        kind: WorkspaceDocKind,
        request: DocUpdateRequest,
    ) -> TalosResult<WorkspaceDocInfo> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
        self.resolver.write_profile_snapshot(&workspace).await?;
        if kind == WorkspaceDocKind::Memory {
            let snapshot = self
                .memory
                .replace_document(
                    &memory_scope(&workspace),
                    request.content,
                    request.base_version,
                )
                .await?;
            return Ok(WorkspaceDocInfo {
                kind,
                path: kind.file_name().to_string(),
                exists: true,
                updated_at: snapshot.updated_at(),
                version_token: snapshot.version_token().map(str::to_string),
                content: snapshot.raw_content().to_string(),
            });
        }
        let path = self.doc_path(&workspace.config, kind);
        talos_fs::with_file_lock(&path, || async {
            let existing = talos_fs::read_file(&path).await?;
            ensure_workspace_write_version(existing.as_deref(), &request)?;
            talos_fs::write_file(&path, &request.content).await
        })
        .await?;

        self.get_doc(actor, kind).await
    }

    pub async fn write_doc_with_request(
        &self,
        actor: &Actor,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocInfo> {
        self.put_doc(
            actor,
            request.kind,
            DocUpdateRequest {
                content: request.content,
                base_version: expected_version(request.expected_version),
            },
        )
        .await
    }

    fn doc_path(&self, config: &TalosConfig, kind: WorkspaceDocKind) -> PathBuf {
        resolve_workspace_root(config).join(kind.file_name())
    }
}

fn memory_scope(workspace: &crate::workspace::EffectiveUserWorkspace) -> MemoryScope {
    MemoryScope {
        workspace_id: workspace.workspace_id.clone(),
        user_id: workspace.user_id.clone(),
        workspace_root: workspace.user_workspace_dir.clone(),
    }
}

fn supported_doc_kinds() -> [WorkspaceDocKind; 5] {
    [
        WorkspaceDocKind::Soul,
        WorkspaceDocKind::Identity,
        WorkspaceDocKind::User,
        WorkspaceDocKind::Heartbeat,
        WorkspaceDocKind::Memory,
    ]
}

fn expected_version(value: Option<String>) -> ExpectedFileVersion {
    match value {
        Some(version_token) => ExpectedFileVersion::Match { version_token },
        None => ExpectedFileVersion::MustNotExist,
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
