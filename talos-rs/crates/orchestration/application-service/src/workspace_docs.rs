use std::path::PathBuf;

use talos_error::{TalosError, TalosResult};
use talos_types::{
    DocUpdateRequest, TalosConfig, WorkspaceDocDetail, WorkspaceDocKind, WorkspaceDocSummary,
};

use crate::access::Actor;
use crate::file::{
    content_hash, modified_at_ms, read_optional, validate_expected_version, with_file_lock,
    workspace_root, write_atomic, FileLockRegistry,
};

#[derive(Clone)]
pub struct WorkspaceDocService {
    base_config: TalosConfig,
    file_locks: FileLockRegistry,
}

impl WorkspaceDocService {
    pub fn new(base_config: TalosConfig, file_locks: FileLockRegistry) -> Self {
        Self {
            base_config,
            file_locks,
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

    pub async fn list_docs(&self) -> TalosResult<Vec<WorkspaceDocSummary>> {
        let mut docs = Vec::new();
        for kind in supported_doc_kinds() {
            let detail = self.get_doc(kind).await?;
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

    pub async fn get_doc(&self, kind: WorkspaceDocKind) -> TalosResult<WorkspaceDocDetail> {
        let path = self.doc_path(kind);
        let content = read_optional(&path).await?;
        let updated_at = modified_at_ms(&path).await?;
        match content {
            Some(content) => Ok(WorkspaceDocDetail {
                kind,
                path: kind.file_name().to_string(),
                exists: true,
                updated_at,
                version_token: Some(content_hash(&content)),
                content,
            }),
            None => Ok(WorkspaceDocDetail {
                kind,
                path: kind.file_name().to_string(),
                exists: false,
                updated_at: None,
                version_token: None,
                content: String::new(),
            }),
        }
    }

    pub async fn put_doc(
        &self,
        _actor: &Actor,
        kind: WorkspaceDocKind,
        request: DocUpdateRequest,
    ) -> TalosResult<WorkspaceDocDetail> {
        let path = self.doc_path(kind);
        with_file_lock(&self.file_locks, &path, async {
            let existing = read_optional(&path).await?;
            validate_expected_version(existing.as_deref(), &request)?;
            write_atomic(&path, &request.content).await
        })
        .await?;

        self.get_doc(kind).await
    }

    fn doc_path(&self, kind: WorkspaceDocKind) -> PathBuf {
        workspace_root(&self.base_config).join(kind.file_name())
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

#[cfg(test)]
mod tests {
    use super::WorkspaceDocService;
    use crate::access::Actor;
    use crate::file::FileLockRegistry;
    use talos_error::TalosError;
    use talos_types::{DocUpdateRequest, ExpectedFileVersion, TalosConfig, WorkspaceDocKind};

    fn test_service() -> (WorkspaceDocService, std::path::PathBuf) {
        let workspace_dir =
            std::env::temp_dir().join(format!("talos-workspace-doc-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
        let mut config = TalosConfig::default();
        config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        (
            WorkspaceDocService::new(config, FileLockRegistry::default()),
            workspace_dir,
        )
    }

    #[tokio::test]
    async fn put_doc_creates_missing_doc() {
        let (service, workspace_dir) = test_service();

        let detail = service
            .put_doc(
                &Actor::full("admin"),
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
            std::fs::read_to_string(workspace_dir.join("SOUL.md"))
                .expect("workspace doc should exist on disk"),
            "soul content"
        );
        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[tokio::test]
    async fn put_doc_rejects_stale_version() {
        let (service, workspace_dir) = test_service();
        std::fs::write(workspace_dir.join("IDENTITY.md"), "current version")
            .expect("seed doc should write");

        let error = service
            .put_doc(
                &Actor::full("admin"),
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
        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[tokio::test]
    async fn list_docs_returns_supported_kinds() {
        let (service, workspace_dir) = test_service();

        let docs = service
            .list_docs()
            .await
            .expect("list workspace docs should succeed");

        assert_eq!(docs.len(), 5);
        assert_eq!(docs[0].kind, WorkspaceDocKind::Soul);
        assert_eq!(docs[4].kind, WorkspaceDocKind::Memory);
        let _ = std::fs::remove_dir_all(workspace_dir);
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
        let _ = std::fs::remove_dir_all(workspace_dir);
    }
}
