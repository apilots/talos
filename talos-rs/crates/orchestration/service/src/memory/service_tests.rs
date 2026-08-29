use std::sync::Arc;

use super::{date_in_range, MemoryDate, MemoryService};
use crate::access::Actor;
use chrono::NaiveDate;
use talos_memory::{
    DreamMemoryDate, DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryEntry,
    DreamMemoryFormationStore, DreamMemoryProvenance, MemoryAgentId, MemoryId, MemoryScope,
    MemoryTimezone, MemoryUserId,
};
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, MemoryFileListRequest, MemoryImportance, MemoryKind,
    TalosConfig,
};

fn dream_memory(date: &str, entries: &[(&str, &str, &str)]) -> String {
    let timezone = talos_config::parse_timezone("UTC").expect("timezone");
    let context = DreamMemoryDocumentContext::new(
        MemoryAgentId::parse("default").expect("agent id"),
        MemoryUserId::parse("user-1").expect("user id"),
        DreamMemoryDate::parse(date).expect("date"),
        MemoryTimezone::new(timezone),
    );
    let occurred_at = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .expect("date")
        .and_hms_opt(9, 30, 0)
        .expect("time")
        .and_utc()
        .timestamp();
    let mut document =
        DreamMemoryDocument::new(context, occurred_at).expect("Dream Memory document");
    for (memory_id, title, body) in entries {
        document
            .create_entry(
                DreamMemoryEntry::builder()
                    .memory_id(MemoryId::parse(*memory_id).expect("memory id"))
                    .title(*title)
                    .body(*body)
                    .occurred_at(occurred_at)
                    .kind(MemoryKind::Episode)
                    .importance(MemoryImportance::Normal)
                    .provenance(DreamMemoryProvenance::UserAuthored)
                    .build()
                    .expect("entry"),
            )
            .expect("create entry");
    }
    document.render().expect("render Dream Memory")
}

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
}

#[tokio::test]
async fn dream_markdown_is_written_to_the_actor_workspace() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config =
        TalosConfig::default().with_workspace_definition(talos_config::WorkspaceDefinition::new(
            talos_config::DEFAULT_WORKSPACE_ID,
            workspace.path().to_path_buf(),
        ));
    config.memory.memory_dirname = "journal".to_string();
    let service = MemoryService::new(
        config,
        Arc::new(
            talos_store::memory::MemoryStore::in_memory()
                .await
                .expect("memory store"),
        ),
    )
    .expect("memory service");

    let content = dream_memory(
        "2026-07-17",
        &[
            (
                "mem_0123456789abcdef0123456789abcdef",
                "User note",
                "personal memory",
            ),
            (
                "mem_11111111111111111111111111111111",
                "Plain heading",
                "user-authored memory",
            ),
        ],
    );
    let detail = service
        .put_file(
            &Actor::regular("user-1"),
            MemoryDate::parse("2026-07-17").expect("date"),
            DocUpdateRequest {
                content: content.clone(),
                base_version: ExpectedFileVersion::MustNotExist,
            },
        )
        .await
        .expect("write Dream Memory");

    assert_eq!(detail.section_count, 2);
    let snapshot = DreamMemoryFormationStore::read_day(
        &service,
        &MemoryScope {
            workspace_id: "default".to_string(),
            user_id: "user-1".to_string(),
            workspace_root: workspace.path().join("user-1"),
        },
        DreamMemoryDate::parse("2026-07-17").expect("date"),
    )
    .await
    .expect("read typed day snapshot");
    assert_eq!(snapshot.entries().len(), 2);
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("user-1/journal/2026-07-17.md"))
            .expect("read user Dream Memory"),
        content
    );
}

#[tokio::test]
async fn file_list_reads_only_the_requested_latest_files() {
    let workspace = tempfile::tempdir().expect("workspace");
    let config =
        TalosConfig::default().with_workspace_definition(talos_config::WorkspaceDefinition::new(
            talos_config::DEFAULT_WORKSPACE_ID,
            workspace.path().to_path_buf(),
        ));
    let service = MemoryService::new(
        config,
        Arc::new(
            talos_store::memory::MemoryStore::in_memory()
                .await
                .expect("memory store"),
        ),
    )
    .expect("memory service");
    let actor = Actor::regular("user-1");
    service
        .put_file(
            &actor,
            MemoryDate::parse("2026-07-18").expect("date"),
            DocUpdateRequest {
                content: dream_memory(
                    "2026-07-18",
                    &[(
                        "mem_0123456789abcdef0123456789abcdef",
                        "Latest note",
                        "Durable memory.",
                    )],
                ),
                base_version: ExpectedFileVersion::MustNotExist,
            },
        )
        .await
        .expect("write latest memory");
    let memory_dir = workspace.path().join("user-1/memory");
    std::fs::write(memory_dir.join("2026-07-17.md"), "invalid older memory")
        .expect("write older memory");

    let response = service
        .list_files_with_request(
            &actor,
            MemoryFileListRequest {
                limit: Some(1),
                ..MemoryFileListRequest::default()
            },
        )
        .await
        .expect("list latest memory only");

    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].date, "2026-07-18");
}
