use super::WorkspaceDocService;
use crate::access::Actor;
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

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
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

#[tokio::test]
async fn put_doc_rejects_oversized_memory() {
    let (service, workspace_dir) = test_service();
    let mut content = talos_memory::UserWorkspaceMemoryDocument::template()
        .expect("template should build")
        .render()
        .expect("template should render");
    content.push_str(&"x".repeat(talos_types::MEMORY_FILE_MAX_CHARS));

    let error = service
        .put_doc(
            &Actor::regular("member"),
            WorkspaceDocKind::Memory,
            DocUpdateRequest {
                content,
                base_version: ExpectedFileVersion::MustNotExist,
            },
        )
        .await
        .expect_err("oversized memory should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    let _ = talos_fs::remove_directory(&workspace_dir).await;
}

#[tokio::test]
async fn memory_doc_write_requires_canonical_okf_but_read_preserves_invalid_raw_content() {
    let (service, workspace_dir) = test_service();
    let actor = Actor::regular("member");
    let user_dir = workspace_dir.join("users").join("member");
    talos_fs::create_directory(&user_dir, true).expect("user workspace should exist");
    talos_fs::write_file(&user_dir.join("MEMORY.md"), "legacy memory")
        .await
        .expect("legacy memory should write");

    let read = service
        .get_doc(&actor, WorkspaceDocKind::Memory)
        .await
        .expect("invalid memory remains readable");
    assert_eq!(read.content, "legacy memory");

    let error = service
        .put_doc(
            &actor,
            WorkspaceDocKind::Memory,
            DocUpdateRequest {
                content: "arbitrary markdown".to_string(),
                base_version: ExpectedFileVersion::Match {
                    version_token: read.version_token.expect("raw revision should exist"),
                },
            },
        )
        .await
        .expect_err("non-canonical memory should be rejected");
    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    let _ = talos_fs::remove_directory(&workspace_dir).await;
}
