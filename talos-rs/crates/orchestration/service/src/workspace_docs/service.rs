use std::path::PathBuf;

use talos_error::{TalosError, TalosResult};
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, TalosConfig, WorkspaceDocInfo, WorkspaceDocKind,
    WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocSummary, WorkspaceDocWriteRequest, WorkspaceInfoRequest, WorkspaceInfoResponse,
};

use crate::access::Actor;
use crate::workspace::file::{
    ensure_workspace_write_version, resolve_workspace_root, workspace_revision,
};
use crate::workspace::{AgentWorkspaceInfo, EffectiveWorkspaceResolver};

#[derive(Clone)]
pub struct WorkspaceDocService {
    resolver: EffectiveWorkspaceResolver,
}

impl WorkspaceDocService {
    pub fn new(base_config: TalosConfig) -> Self {
        Self {
            resolver: EffectiveWorkspaceResolver::new(base_config),
        }
    }

    pub fn parse_kind(raw: &str) -> TalosResult<WorkspaceDocKind> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "soul" => Ok(WorkspaceDocKind::Soul),
            "identity" => Ok(WorkspaceDocKind::Identity),
            "user" => Ok(WorkspaceDocKind::User),
            "heartbeat" => Ok(WorkspaceDocKind::Heartbeat),
            "memory" => Ok(WorkspaceDocKind::Memory),
            _ => Err(TalosError::ValidationError(
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

    pub fn workspace_info(&self, actor: &Actor) -> TalosResult<AgentWorkspaceInfo> {
        Ok(self.resolver.resolve_for_actor(actor)?.into())
    }

    pub fn workspace_info_with_request(
        &self,
        actor: &Actor,
        _request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        let info = self.workspace_info(actor)?;
        Ok(WorkspaceInfoResponse {
            user_id: info.user_id,
            username: info.username,
            base_workspace_dir: info.base_workspace_dir,
            user_workspace_dir: info.user_workspace_dir,
        })
    }

    pub async fn get_doc(
        &self,
        actor: &Actor,
        kind: WorkspaceDocKind,
    ) -> TalosResult<WorkspaceDocInfo> {
        let workspace = self.resolver.resolve_for_actor(actor)?;
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
mod tests {
    use super::WorkspaceDocService;
    use crate::access::Actor;
    use talos_error::TalosError;
    use talos_types::{DocUpdateRequest, ExpectedFileVersion, TalosConfig, WorkspaceDocKind};

    fn test_service() -> (WorkspaceDocService, std::path::PathBuf) {
        let workspace_dir =
            std::env::temp_dir().join(format!("talos-workspace-doc-test-{}", uuid::Uuid::new_v4()));
        talos_fs::create_directory(&workspace_dir, true).expect("workspace dir should exist");
        let mut config = TalosConfig::default();
        config.agent.kernel.workspace_dir = workspace_dir.to_string_lossy().to_string();
        (WorkspaceDocService::new(config), workspace_dir)
    }

    #[tokio::test]
    async fn put_doc_creates_missing_doc() {
        let (service, workspace_dir) = test_service();

        let detail = service
            .put_doc(
                &Actor::regular("admin"),
                WorkspaceDocKind::Soul,
                DocUpdateRequest {
                    content: "soul content".to_string(),
                    base_version: ExpectedFileVersion::MustNotExist,
                },
            )
            .await
            .expect("put workspace doc should succeed");

        assert!(detail.exists);
        assert_eq!(detail.content, "soul content");
        assert_eq!(
            talos_fs::read_file(&workspace_dir.join("users").join("admin").join("SOUL.md"))
                .await
                .expect("workspace doc should read")
                .expect("workspace doc should exist on disk"),
            "soul content"
        );
        let _ = talos_fs::remove_directory(&workspace_dir).await;
    }

    #[tokio::test]
    async fn put_doc_rejects_stale_version() {
        let (service, workspace_dir) = test_service();
        let user_dir = workspace_dir.join("users").join("admin");
        talos_fs::create_directory(&user_dir, true).expect("user workspace should exist");
        talos_fs::write_file(&user_dir.join("IDENTITY.md"), "current version")
            .await
            .expect("seed doc should write");

        let error = service
            .put_doc(
                &Actor::regular("admin"),
                WorkspaceDocKind::Identity,
                DocUpdateRequest {
                    content: "updated".to_string(),
                    base_version: ExpectedFileVersion::Match {
                        version_token: "stale".to_string(),
                    },
                },
            )
            .await
            .expect_err("stale version should be rejected");

        assert!(matches!(error, TalosError::Conflict(_)));
        let _ = talos_fs::remove_directory(&workspace_dir).await;
    }

    #[tokio::test]
    async fn list_docs_returns_supported_kinds() {
        let (service, workspace_dir) = test_service();

        let docs = service
            .list_docs(&Actor::regular("member"))
            .await
            .expect("list workspace docs should succeed");

        assert_eq!(docs.len(), 5);
        assert_eq!(docs[0].kind, WorkspaceDocKind::Soul);
        assert_eq!(docs[4].kind, WorkspaceDocKind::Memory);
        let _ = talos_fs::remove_directory(&workspace_dir).await;
    }

    #[tokio::test]
    async fn put_doc_allows_regular_actor() {
        let (service, workspace_dir) = test_service();

        let detail = service
            .put_doc(
                &Actor::regular("member"),
                WorkspaceDocKind::Soul,
                DocUpdateRequest {
                    content: "# Shared doc".to_string(),
                    base_version: ExpectedFileVersion::MustNotExist,
                },
            )
            .await
            .expect("regular actor should write own workspace docs");

        assert_eq!(detail.content, "# Shared doc");
        let _ = talos_fs::remove_directory(&workspace_dir).await;
    }
}
