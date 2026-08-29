use std::sync::Arc;

use talos_memory::{
    MemoryAgentId, MemoryScope, OptionalField, UserWorkspaceMemoryEntry,
    UserWorkspaceMemoryEntryQuery, UserWorkspaceMemoryEntryUpdate, UserWorkspaceMemoryProducer,
    UserWorkspaceMemorySection, UserWorkspaceMemorySnapshot,
};
use talos_types::{ExpectedFileVersion, MemoryKind};

use super::UserWorkspaceMemoryService;

fn scope(root: &std::path::Path) -> MemoryScope {
    MemoryScope {
        workspace_id: "default".to_string(),
        user_id: "user-1".to_string(),
        workspace_root: root.to_path_buf(),
    }
}

fn entry(key: &str, summary: &str) -> UserWorkspaceMemoryEntry {
    UserWorkspaceMemoryEntry::builder()
        .key(key)
        .section(UserWorkspaceMemorySection::KeyKnowledge)
        .title(format!("Entry {key}"))
        .kind(MemoryKind::Fact)
        .summary(summary)
        .build()
        .expect("entry should build")
}

#[tokio::test]
async fn missing_read_returns_in_memory_template_without_creating_file() {
    let directory = tempfile::tempdir().expect("temporary directory should create");
    let service = UserWorkspaceMemoryService::new();
    let scope = scope(directory.path());

    let snapshot = service.read(&scope).await.expect("snapshot should read");

    assert!(matches!(
        &snapshot,
        UserWorkspaceMemorySnapshot::Missing { .. }
    ));
    assert_eq!(snapshot.raw_content(), "");
    assert!(snapshot.document().is_some());
    assert!(!directory.path().join("MEMORY.md").exists());
}

#[tokio::test]
async fn invalid_file_requires_versioned_replace_before_entry_crud() {
    let directory = tempfile::tempdir().expect("temporary directory should create");
    let service = UserWorkspaceMemoryService::new();
    let scope = scope(directory.path());
    talos_fs::write_file(&directory.path().join("MEMORY.md"), "legacy memory")
        .await
        .expect("legacy file should write");

    let invalid = service.read(&scope).await.expect("snapshot should read");
    assert!(matches!(
        &invalid,
        UserWorkspaceMemorySnapshot::Invalid { .. }
    ));
    service
        .upsert_entry(&scope, entry("blocked", "must not write"))
        .await
        .expect_err("entry CRUD should reject invalid documents");

    let replacement =
        talos_memory::UserWorkspaceMemoryDocument::template(UserWorkspaceMemoryProducer::new(
            MemoryAgentId::parse("agent-1").expect("agent id"),
            1_784_251_800,
        ))
        .expect("template should build")
        .render()
        .expect("template should render");
    let repaired = service
        .replace_document(
            &scope,
            replacement,
            ExpectedFileVersion::Match {
                version_token: invalid
                    .version_token()
                    .expect("invalid file has a revision")
                    .to_string(),
            },
        )
        .await
        .expect("replace should repair the file");
    assert!(matches!(
        repaired,
        UserWorkspaceMemorySnapshot::Valid { .. }
    ));
}

#[tokio::test]
async fn concurrent_upserts_preserve_unrelated_keys() {
    let directory = tempfile::tempdir().expect("temporary directory should create");
    let service = Arc::new(UserWorkspaceMemoryService::new());
    let scope = Arc::new(scope(directory.path()));

    let left = {
        let service = Arc::clone(&service);
        let scope = Arc::clone(&scope);
        tokio::spawn(async move {
            service
                .upsert_entry(&scope, entry("left", "left value"))
                .await
        })
    };
    let right = {
        let service = Arc::clone(&service);
        let scope = Arc::clone(&scope);
        tokio::spawn(async move {
            service
                .upsert_entry(&scope, entry("right", "right value"))
                .await
        })
    };
    left.await
        .expect("left task should join")
        .expect("left write should succeed");
    right
        .await
        .expect("right task should join")
        .expect("right write should succeed");

    let snapshot = service.read(&scope).await.expect("snapshot should read");
    let document = snapshot.document().expect("document should be valid");
    assert!(document.get_entry("left").is_some());
    assert!(document.get_entry("right").is_some());
}

#[tokio::test]
async fn stale_replace_does_not_modify_existing_file() {
    let directory = tempfile::tempdir().expect("temporary directory should create");
    let service = UserWorkspaceMemoryService::new();
    let scope = scope(directory.path());
    let first = service
        .upsert_entry(&scope, entry("first", "first value"))
        .await
        .expect("first write should succeed");
    let content = first.raw_content().to_string();

    let error = service
        .replace_document(
            &scope,
            content.clone(),
            ExpectedFileVersion::Match {
                version_token: "stale".to_string(),
            },
        )
        .await
        .expect_err("stale replace should conflict");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        talos_fs::read_file(&directory.path().join("MEMORY.md"))
            .await
            .expect("file should read")
            .expect("file should exist"),
        content
    );
}

#[tokio::test]
async fn typed_service_api_supports_complete_entry_crud() {
    let directory = tempfile::tempdir().expect("temporary directory should create");
    let service = UserWorkspaceMemoryService::new();
    let scope = scope(directory.path());

    service
        .create_entry(&scope, entry("memory-api", "created"))
        .await
        .expect("entry should create");
    assert_eq!(
        service
            .get_entry(&scope, "memory-api")
            .await
            .expect("entry should read")
            .expect("entry should exist")
            .summary,
        "created"
    );

    service
        .update_entry(
            &scope,
            "memory-api",
            UserWorkspaceMemoryEntryUpdate::builder()
                .summary("updated")
                .why(OptionalField::Set("durable reason".to_string()))
                .build()
                .expect("update should build"),
        )
        .await
        .expect("entry should update");
    assert_eq!(
        service
            .list_entries(
                &scope,
                UserWorkspaceMemoryEntryQuery::builder()
                    .key_prefix("memory-")
                    .build()
                    .expect("query should build"),
            )
            .await
            .expect("entries should list")
            .len(),
        1
    );

    service
        .replace_entry(&scope, entry("memory-api", "replaced"))
        .await
        .expect("entry should replace");
    assert_eq!(
        service
            .get_entry(&scope, "memory-api")
            .await
            .expect("entry should read")
            .expect("entry should exist")
            .summary,
        "replaced"
    );

    service
        .remove_entry(&scope, "memory-api")
        .await
        .expect("entry should remove");
    service
        .remove_entry(&scope, "memory-api")
        .await
        .expect("remove should be idempotent");
    assert!(service
        .get_entry(&scope, "memory-api")
        .await
        .expect("entry should read")
        .is_none());
}

#[tokio::test]
async fn service_keeps_distinct_effective_workspace_scopes_isolated() {
    let first = tempfile::tempdir().expect("first directory should create");
    let second = tempfile::tempdir().expect("second directory should create");
    let service = UserWorkspaceMemoryService::new();
    let first_scope = scope(first.path());
    let second_scope = MemoryScope {
        workspace_id: "default".to_string(),
        user_id: "user-2".to_string(),
        workspace_root: second.path().to_path_buf(),
    };

    service
        .upsert_entry(&first_scope, entry("private-fact", "first scope only"))
        .await
        .expect("entry should write");

    assert!(service
        .get_entry(&second_scope, "private-fact")
        .await
        .expect("second scope should read")
        .is_none());
    assert!(!second.path().join("MEMORY.md").exists());
}
