use std::sync::Arc;

use super::{date_in_range, MemoryDate, MemoryService};
use crate::access::Actor;
use talos_types::{DocUpdateRequest, ExpectedFileVersion, MemoryFileListRequest, TalosConfig};

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
async fn daily_markdown_is_written_to_the_actor_workspace() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = workspace.path().to_string_lossy().to_string();
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

    let detail = service
        .put_file(
            &Actor::regular("user-1"),
            MemoryDate::parse("2026-07-17").expect("date"),
            DocUpdateRequest {
                content: "## [09:30] User note\n\n- personal memory\n\n## Plain heading\n\n- legacy memory\n"
                    .to_string(),
                base_version: ExpectedFileVersion::MustNotExist,
            },
        )
        .await
        .expect("write daily memory");

    assert_eq!(detail.section_count, 2);
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("users/user-1/journal/2026-07-17.md"))
            .expect("read user daily memory"),
        "## [09:30] User note\n\n- personal memory\n\n## Plain heading\n\n- legacy memory\n"
    );
}

#[tokio::test]
async fn identical_daily_write_retries_index_sync() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = workspace.path().to_string_lossy().to_string();
    let service = MemoryService::new(
        config,
        Arc::new(
            talos_store::memory::MemoryStore::in_memory()
                .await
                .expect("memory store"),
        ),
    )
    .expect("memory service");
    let request = DocUpdateRequest {
        content: "## Retry-safe note\n\n- durable memory\n".to_string(),
        base_version: ExpectedFileVersion::MustNotExist,
    };
    let actor = Actor::regular("user-1");
    let date = MemoryDate::parse("2026-07-18").expect("date");
    service
        .put_file(&actor, date, request.clone())
        .await
        .expect("initial write");

    let detail = service
        .put_file(&actor, date, request)
        .await
        .expect("identical retry should resync the index");

    assert_eq!(detail.section_count, 1);
}

#[tokio::test]
async fn file_list_reads_only_the_requested_latest_files() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = workspace.path().to_string_lossy().to_string();
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
                content: "## Latest note\n\nDurable memory.\n".to_string(),
                base_version: ExpectedFileVersion::MustNotExist,
            },
        )
        .await
        .expect("write latest memory");
    let memory_dir = workspace.path().join("users/user-1/memory");
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
