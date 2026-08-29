use talos_memory::{
    DreamMemoryDate, DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryEntry,
    DreamMemoryProvenance, DreamMemorySourceRange, MemoryAgentId, MemoryId, MemorySessionId,
    MemoryTimezone, MemoryUserId,
};
use talos_types::{MemoryImportance, MemoryKind};

use super::{
    convert_legacy_scope, switch_memory_directories, verify_source_revisions, workspace_revision,
    SourceRevision,
};

fn timezone() -> talos_config::TalosTimezone {
    talos_config::parse_timezone("UTC").expect("timezone")
}

#[tokio::test]
async fn conversion_preserves_generated_data_and_converts_plain_entries() {
    let workspace = tempfile::tempdir().expect("workspace");
    let user_workspace = workspace.path().join("user-1");
    let memory_dir = user_workspace.join("memory");
    talos_fs::create_directory(&memory_dir, true).expect("memory directory");
    let legacy = r#"## [09:30] Generated memory
memory_id: mem_0123456789abcdef0123456789abcdef
occurred_at: 1784251800
session: session-1
kind: fact
importance: medium
source_from_seq: 12
source_until_seq: 18

Generated body.

## Plain memory
session: partial-session

User-authored body.
"#;
    talos_fs::write_file(&memory_dir.join("2026-07-17.md"), legacy)
        .await
        .expect("legacy source");
    talos_fs::write_file(
        &memory_dir.join("2026-07-18.md"),
        "Unstructured content previously ignored by the legacy index.\n",
    )
    .await
    .expect("unstructured legacy source");
    let backup_dir = user_workspace.join(".memory.legacy-backup");
    let staging_dir = user_workspace.join(".memory.dream-memory-okf-staging");
    talos_fs::rename(&memory_dir, &backup_dir).expect("simulate first switch rename");
    talos_fs::create_directory(&staging_dir, true).expect("staging directory");
    talos_fs::write_file(&staging_dir.join("partial.md"), "partial")
        .await
        .expect("partial staging file");

    convert_legacy_scope("default", "user-1", &user_workspace, "memory", timezone())
        .await
        .expect("convert known scope")
        .expect("legacy conversion")
        .finish()
        .await
        .expect("finish conversion");
    let content = talos_fs::read_file(&memory_dir.join("2026-07-17.md"))
        .await
        .expect("read canonical source")
        .expect("canonical source");
    let context = DreamMemoryDocumentContext::new(
        MemoryAgentId::parse("default").expect("agent id"),
        MemoryUserId::parse("user-1").expect("user id"),
        DreamMemoryDate::parse("2026-07-17").expect("date"),
        MemoryTimezone::new(timezone()),
    );
    let document = DreamMemoryDocument::parse(context, &content).expect("canonical document");

    let expected = vec![
        DreamMemoryEntry::builder()
            .memory_id(
                MemoryId::parse("mem_0123456789abcdef0123456789abcdef").expect("generated id"),
            )
            .title("Generated memory")
            .body("Generated body.")
            .occurred_at(1_784_251_800)
            .kind(MemoryKind::Fact)
            .importance(MemoryImportance::Normal)
            .provenance(DreamMemoryProvenance::Generated {
                session_id: MemorySessionId::parse("session-1").expect("session id"),
                source_range: DreamMemorySourceRange::new(12, 18).expect("source range"),
            })
            .build()
            .expect("generated entry"),
        DreamMemoryEntry::builder()
            .memory_id(
                MemoryId::parse("mem_13a892fe5fe9e70069c5ba816948ecae").expect("user-authored id"),
            )
            .title("Plain memory")
            .body("User-authored body.")
            .occurred_at(1_784_246_400)
            .kind(MemoryKind::Episode)
            .importance(MemoryImportance::Normal)
            .provenance(DreamMemoryProvenance::UserAuthored)
            .build()
            .expect("user-authored entry"),
    ];
    assert_eq!(document.entries(), expected);
    let empty_content = talos_fs::read_file(&memory_dir.join("2026-07-18.md"))
        .await
        .expect("read converted empty source")
        .expect("converted empty source");
    let empty_context = DreamMemoryDocumentContext::new(
        MemoryAgentId::parse("default").expect("agent id"),
        MemoryUserId::parse("user-1").expect("user id"),
        DreamMemoryDate::parse("2026-07-18").expect("date"),
        MemoryTimezone::new(timezone()),
    );
    assert!(DreamMemoryDocument::parse(empty_context, &empty_content)
        .expect("empty canonical document")
        .entries()
        .is_empty());
    assert!(!backup_dir.exists());
    assert!(!staging_dir.exists());
}

#[tokio::test]
async fn invalid_source_does_not_switch_the_directory() {
    let workspace = tempfile::tempdir().expect("workspace");
    let memory_dir = workspace.path().join("memory");
    talos_fs::create_directory(&memory_dir, true).expect("memory directory");
    let source_path = memory_dir.join("2026-07-17.md");
    let invalid_sources = [
        "## [09:30] Invalid\nmemory_id: mem_0123456789abcdef0123456789abcdef\n\nBody.\n",
        "---\ntype: DreamMemory\ntitle: Dream Memory 2026-07-17\n---\n\n# Dream Memory 2026-07-17\n\n## Memories\n",
    ];
    for source in invalid_sources {
        talos_fs::write_file(&source_path, source)
            .await
            .expect("invalid source");

        convert_legacy_scope("default", "user-1", workspace.path(), "memory", timezone())
            .await
            .expect_err("invalid conversion must fail");

        assert_eq!(
            talos_fs::read_file(&source_path)
                .await
                .expect("read unchanged source"),
            Some(source.to_string())
        );
    }
    assert!(!workspace.path().join(".memory.legacy-backup").exists());
}

#[tokio::test]
async fn revision_guard_rejects_source_content_and_file_set_changes() {
    let workspace = tempfile::tempdir().expect("workspace");
    let memory_dir = workspace.path().join("memory");
    talos_fs::create_directory(&memory_dir, true).expect("memory directory");
    let source_path = memory_dir.join("2026-07-17.md");
    let original = "## Legacy\n\nOriginal body.\n";
    talos_fs::write_file(&source_path, original)
        .await
        .expect("legacy source");
    let sources = vec![SourceRevision {
        path: source_path.clone(),
        version: workspace_revision(original),
    }];

    talos_fs::write_file(&source_path, "## Legacy\n\nChanged body.\n")
        .await
        .expect("changed legacy source");
    let changed = verify_source_revisions(&memory_dir, &sources)
        .await
        .expect_err("content revision change must fail");

    talos_fs::write_file(&source_path, original)
        .await
        .expect("restore legacy source");
    talos_fs::write_file(&memory_dir.join("2026-07-18.md"), "## New source\n")
        .await
        .expect("new legacy source");
    let file_set = verify_source_revisions(&memory_dir, &sources)
        .await
        .expect_err("file set change must fail");

    assert!(changed.is_code(talos_error::ErrorCode::Conflict));
    assert!(file_set.is_code(talos_error::ErrorCode::Conflict));
}

#[tokio::test]
async fn failed_staging_switch_restores_the_legacy_directory() {
    let workspace = tempfile::tempdir().expect("workspace");
    let memory_dir = workspace.path().join("memory");
    let staging_dir = workspace.path().join(".memory.dream-memory-okf-staging");
    let backup_dir = workspace.path().join(".memory.legacy-backup");
    talos_fs::create_directory(&memory_dir, true).expect("memory directory");
    let source_path = memory_dir.join("2026-07-17.md");
    let legacy = "## Legacy\n\nOriginal body.\n";
    talos_fs::write_file(&source_path, legacy)
        .await
        .expect("legacy source");

    switch_memory_directories(&memory_dir, &staging_dir, &backup_dir)
        .expect_err("missing staging directory must fail the second rename");

    assert_eq!(
        talos_fs::read_file(&source_path)
            .await
            .expect("read restored source"),
        Some(legacy.to_string())
    );
    assert!(!backup_dir.exists());
}
