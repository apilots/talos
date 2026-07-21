use std::fs;
use std::time::Instant;

use chrono_tz::UTC;

use super::{MemoryImportance, MemoryKind, MemorySearchQuery, MemoryStore};
use talos_memory::{MemoryCandidate, MemoryWriteBatch};

async fn test_store(root: &std::path::Path) -> MemoryStore {
    let database = root.join("memory-index.sqlite");
    MemoryStore::open(&database).await.expect("memory store")
}

fn write_memory_file(root: &std::path::Path, date: &str, content: &str) {
    let memory_dir = root.join("memory");
    fs::create_dir_all(&memory_dir).expect("memory directory");
    fs::write(memory_dir.join(format!("{date}.md")), content).expect("memory file");
}

#[tokio::test]
async fn rebuild_rejects_source_sequences_outside_sqlite_integer_bounds() {
    let workspace = tempfile::tempdir().expect("workspace");
    let content = format!(
        "## [09:30] Invalid range\n\
memory_id: mem_0123456789abcdef0123456789abcdef\n\
occurred_at: 1784251800\n\
session: session-1\n\
kind: fact\n\
importance: normal\n\
source_from_seq: {}\n\
source_until_seq: {}\n\n\
Invalid range.\n",
        i64::MAX as u64 + 1,
        i64::MAX as u64 + 1,
    );
    write_memory_file(workspace.path(), "2026-07-17", &content);
    let store = test_store(workspace.path()).await;

    let error = store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect_err("out-of-range sequence must fail");

    assert!(matches!(error, talos_error::TalosError::ValidationError(_)));
}

fn memory_batch(
    session_id: &str,
    occurred_at: i64,
    source_from_seq: u64,
    source_until_seq: u64,
    body: &str,
) -> MemoryWriteBatch {
    MemoryWriteBatch {
        session_id: session_id.to_string(),
        occurred_at,
        source_from_seq,
        source_until_seq,
        candidates: vec![MemoryCandidate {
            title: "Durable memory".to_string(),
            body: body.to_string(),
            kind: MemoryKind::Fact,
            importance: MemoryImportance::Normal,
        }],
    }
}

#[tokio::test]
async fn rebuild_search_and_get_use_markdown_as_source() {
    let workspace = tempfile::tempdir().expect("workspace");
    let content = r#"## [09:30] Durable preference
memory_id: mem_0123456789abcdef0123456789abcdef
occurred_at: 1784251800
session: session-1
kind: preference
importance: high
source_from_seq: 12
source_until_seq: 18

- The user prefers concise architecture proposals.

## [10:00] Existing note
session: legacy-session

- SQLite can be rebuilt from Markdown.

## [unknown] Untimed note

- Midnight default memory.
"#;
    write_memory_file(workspace.path(), "2026-07-17", content);
    let store = test_store(workspace.path()).await;

    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("first rebuild");
    let first = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "concise architecture".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("search");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].memory.kind, MemoryKind::Preference);
    assert_eq!(first[0].memory.importance, MemoryImportance::High);
    assert_eq!(first[0].rank, 1);
    assert_eq!(
        store
            .get("user-1", &first[0].memory.memory_id)
            .await
            .expect("get memory"),
        Some(first[0].memory.clone())
    );

    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("repeat rebuild");
    let legacy_query = MemorySearchQuery {
        text: "SQLite".to_string(),
        limit: 6,
        kinds: None,
        since_at: None,
        until_at: None,
    };
    let legacy_id = store
        .search("user-1", &legacy_query)
        .await
        .expect("legacy search")[0]
        .memory
        .memory_id
        .clone();
    let untimed = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "Midnight default".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("untimed legacy search");
    assert_eq!(untimed[0].memory.session_id, None);
    assert_eq!(untimed[0].memory.occurred_at, 1_784_246_400);
    assert_eq!(
        fs::read_to_string(workspace.path().join("memory/2026-07-17.md")).expect("read source"),
        content
    );

    drop(store);
    fs::remove_file(workspace.path().join("memory-index.sqlite")).expect("remove derived index");
    let rebuilt_store = test_store(workspace.path()).await;
    rebuilt_store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("rebuild deleted index");
    assert_eq!(
        rebuilt_store
            .search("user-1", &legacy_query)
            .await
            .expect("search rebuilt index")[0]
            .memory
            .memory_id,
        legacy_id
    );
}

#[tokio::test]
async fn rebuild_uses_the_configured_memory_directory() {
    let workspace = tempfile::tempdir().expect("workspace");
    let memory_dir = workspace.path().join("journal");
    fs::create_dir_all(&memory_dir).expect("memory directory");
    fs::write(
        memory_dir.join("2026-07-17.md"),
        "## Configured source\n\nThe configured directory is canonical.\n",
    )
    .expect("memory file");
    let store = test_store(workspace.path()).await;

    store
        .rebuild_scope("user-1", workspace.path(), "journal", UTC)
        .await
        .expect("rebuild configured directory");

    assert_eq!(
        store
            .search(
                "user-1",
                &MemorySearchQuery {
                    text: "configured directory".to_string(),
                    limit: 6,
                    kinds: None,
                    since_at: None,
                    until_at: None,
                },
            )
            .await
            .expect("search configured directory")
            .len(),
        1
    );
}

#[tokio::test]
async fn search_preview_uses_the_same_case_insensitive_match_as_search() {
    let workspace = tempfile::tempdir().expect("workspace");
    let body = format!("{} architecture decision", "prefix ".repeat(80));
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        &format!("## Long note\n\n{body}\n"),
    );
    let store = test_store(workspace.path()).await;
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("rebuild");

    let result = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "ARCHITECTURE".to_string(),
                limit: 1,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("search");

    assert!(result[0].preview.contains("architecture decision"));
}

#[tokio::test]
async fn rebuild_replaces_changed_sources_and_isolates_users() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        "## [09:30] First note\n\n- alpha memory\n",
    );
    let store = test_store(workspace.path()).await;
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("user one rebuild");
    store
        .rebuild_scope("user-2", workspace.path(), "memory", UTC)
        .await
        .expect("user two rebuild");

    write_memory_file(
        workspace.path(),
        "2026-07-17",
        "## [09:30] Second note\n\n- beta memory\n",
    );
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("changed rebuild");

    let search = |text: &str| MemorySearchQuery {
        text: text.to_string(),
        limit: 6,
        kinds: None,
        since_at: None,
        until_at: None,
    };
    assert!(store
        .search("user-1", &search("alpha"))
        .await
        .expect("removed source search")
        .is_empty());
    assert_eq!(
        store
            .search("user-1", &search("beta"))
            .await
            .expect("changed source search")
            .len(),
        1
    );

    fs::remove_file(workspace.path().join("memory/2026-07-17.md")).expect("remove memory source");
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("deleted source rebuild");
    assert!(store
        .search("user-1", &search("beta"))
        .await
        .expect("deleted source search")
        .is_empty());
    assert_eq!(
        store
            .search("user-2", &search("alpha"))
            .await
            .expect("other user search")
            .len(),
        1
    );
}

#[tokio::test]
async fn write_batch_is_idempotent_and_immediately_searchable() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let batch = MemoryWriteBatch {
        session_id: "session-1".to_string(),
        occurred_at: 1_784_251_800,
        source_from_seq: 12,
        source_until_seq: 18,
        candidates: vec![MemoryCandidate {
            title: "Durable preference".to_string(),
            body: "The user prefers concise architecture proposals.".to_string(),
            kind: MemoryKind::Preference,
            importance: MemoryImportance::High,
        }],
    };

    let (first_write, retry_write) = tokio::join!(
        store.write_batch("user-1", workspace.path(), "memory", UTC, &batch),
        store.write_batch("user-1", workspace.path(), "memory", UTC, &batch),
    );
    first_write.expect("write batch");
    retry_write.expect("concurrent retry batch");

    let results = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "concise architecture".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("search written memory");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].memory.source_from_seq, Some(12));
    assert_eq!(
        fs::read_to_string(workspace.path().join("memory/2026-07-17.md"))
            .expect("read written memory")
            .matches("memory_id:")
            .count(),
        1
    );

    drop(store);
    fs::remove_file(workspace.path().join("memory-index.sqlite")).expect("remove derived index");
    let recovered_store = test_store(workspace.path()).await;
    let mut regenerated_batch = batch.clone();
    regenerated_batch.candidates[0].body = "A different regenerated memory.".to_string();
    recovered_store
        .write_batch(
            "user-1",
            workspace.path(),
            "memory",
            UTC,
            &regenerated_batch,
        )
        .await
        .expect("retry uses canonical Markdown after the index sync fails");
    assert_eq!(
        recovered_store
            .search(
                "user-1",
                &MemorySearchQuery {
                    text: "concise architecture".to_string(),
                    limit: 6,
                    kinds: None,
                    since_at: None,
                    until_at: None,
                },
            )
            .await
            .expect("search recovered memory")
            .len(),
        1
    );
    assert!(recovered_store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "different regenerated".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("search regenerated memory")
        .is_empty());
}

#[tokio::test]
async fn write_batch_with_empty_retry_recovers_committed_markdown() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let batch = memory_batch(
        "session-1",
        1_784_251_800,
        12,
        18,
        "Canonical memory survives index failure.",
    );
    store
        .write_batch("user-1", workspace.path(), "memory", UTC, &batch)
        .await
        .expect("write canonical Markdown");

    drop(store);
    fs::remove_file(workspace.path().join("memory-index.sqlite")).expect("remove derived index");
    let recovered_store = test_store(workspace.path()).await;
    let mut empty_retry = batch;
    empty_retry.candidates.clear();
    recovered_store
        .write_batch("user-1", workspace.path(), "memory", UTC, &empty_retry)
        .await
        .expect("empty retry syncs canonical Markdown");

    assert_eq!(
        recovered_store
            .search(
                "user-1",
                &MemorySearchQuery {
                    text: "survives index failure".to_string(),
                    limit: 6,
                    kinds: None,
                    since_at: None,
                    until_at: None,
                },
            )
            .await
            .expect("search recovered memory")
            .len(),
        1
    );
}

#[tokio::test]
async fn write_batch_rejects_partial_range_overlap_before_changing_markdown() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let first = memory_batch("session-1", 1_784_251_800, 10, 20, "First range.");
    store
        .write_batch("user-1", workspace.path(), "memory", UTC, &first)
        .await
        .expect("write first range");
    let path = workspace.path().join("memory/2026-07-17.md");
    let original = fs::read_to_string(&path).expect("read first range");
    let overlapping = memory_batch("session-1", 1_784_251_800, 20, 30, "Overlapping range.");

    let error = store
        .write_batch("user-1", workspace.path(), "memory", UTC, &overlapping)
        .await
        .expect_err("partial overlap must fail");

    assert!(matches!(error, talos_error::TalosError::Conflict(_)));
    assert_eq!(
        fs::read_to_string(path).expect("read unchanged source"),
        original
    );
}

#[tokio::test]
async fn write_batch_rejects_cross_source_duplicate_before_creating_target() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let batch = memory_batch("session-1", 1_784_251_800, 10, 20, "Canonical record.");
    store
        .write_batch("user-1", workspace.path(), "memory", UTC, &batch)
        .await
        .expect("write canonical source");
    let original_path = workspace.path().join("memory/2026-07-17.md");
    let moved_path = workspace.path().join("memory/2026-07-18.md");
    fs::rename(&original_path, &moved_path).expect("move canonical source");
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("index moved source");

    let error = store
        .write_batch("user-1", workspace.path(), "memory", UTC, &batch)
        .await
        .expect_err("cross-source duplicate must fail");

    assert!(matches!(error, talos_error::TalosError::Conflict(_)));
    assert!(!original_path.exists());
}

#[tokio::test]
async fn write_batch_syncs_only_its_target_source() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let first = memory_batch("session-1", 1_784_251_800, 1, 2, "First source record.");
    store
        .write_batch("user-1", workspace.path(), "memory", UTC, &first)
        .await
        .expect("write first record");
    write_memory_file(
        workspace.path(),
        "2026-07-18",
        "## Invalid\nmemory_id: invalid\n\nMalformed unrelated source.\n",
    );
    let second = memory_batch("session-1", 1_784_251_800, 3, 4, "Second source record.");

    store
        .write_batch("user-1", workspace.path(), "memory", UTC, &second)
        .await
        .expect("write without scanning unrelated source");

    assert_eq!(
        fs::read_to_string(workspace.path().join("memory/2026-07-17.md"))
            .expect("read target source")
            .matches("memory_id:")
            .count(),
        2
    );
}

#[tokio::test]
async fn search_applies_filters_weight_and_stable_rank() {
    let workspace = tempfile::tempdir().expect("workspace");
    let content = r#"## [09:30] Architecture preference
memory_id: mem_11111111111111111111111111111111
occurred_at: 1784251800
session: session-1
kind: preference
importance: high
source_from_seq: 1
source_until_seq: 1

The user prefers concise architecture. 中文架构记忆。 The user said "concise".

## [09:31] Architecture episode
memory_id: mem_22222222222222222222222222222222
occurred_at: 1784251800
session: session-2
kind: episode
importance: low
source_from_seq: 2
source_until_seq: 2

The user reviewed concise architecture.
"#;
    write_memory_file(workspace.path(), "2026-07-17", content);
    let store = test_store(workspace.path()).await;
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("rebuild");

    let ranked = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "concise architecture".to_string(),
                limit: 6,
                kinds: None,
                since_at: Some(1784251800),
                until_at: Some(1784251800),
            },
        )
        .await
        .expect("ranked search");
    assert_eq!(
        ranked
            .iter()
            .map(|record| (record.memory.memory_id.as_str(), record.rank))
            .collect::<Vec<_>>(),
        vec![
            ("mem_11111111111111111111111111111111", 1),
            ("mem_22222222222222222222222222222222", 2),
        ]
    );

    let filtered = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "架构".to_string(),
                limit: 6,
                kinds: Some(vec![MemoryKind::Preference]),
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("short filtered search");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].memory.kind, MemoryKind::Preference);
    assert!(filtered[0].preview.chars().count() <= 300);

    let escaped = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "\"concise\"".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("literal quote search");
    assert_eq!(escaped.len(), 1);
}

#[tokio::test]
async fn rebuild_indexes_legacy_plain_headings() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        "## Audit scope\n\n- Preserve existing Markdown memory\n",
    );
    let store = test_store(workspace.path()).await;

    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("legacy plain heading should rebuild");
    let results = store
        .search(
            "user-1",
            &MemorySearchQuery {
                text: "existing Markdown".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("legacy memory should be searchable");

    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn corrupt_index_is_recreated_from_markdown() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        "## [09:30] Recoverable note\n\n- canonical markdown survives\n",
    );
    let database = workspace.path().join("memory-index.sqlite");
    fs::write(&database, b"not a sqlite database").expect("corrupt index");

    let store = MemoryStore::open(&database)
        .await
        .expect("corrupt index should be replaced");
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("rebuild from markdown");

    assert_eq!(
        store
            .search(
                "user-1",
                &MemorySearchQuery {
                    text: "canonical markdown".to_string(),
                    limit: 6,
                    kinds: None,
                    since_at: None,
                    until_at: None,
                },
            )
            .await
            .expect("search rebuilt index")
            .len(),
        1
    );
}

#[tokio::test]
async fn source_edit_keeps_generated_provenance_immutable() {
    let workspace = tempfile::tempdir().expect("workspace");
    let current = r#"## [09:30] Durable preference
memory_id: mem_0123456789abcdef0123456789abcdef
occurred_at: 1784251800
session: session-1
kind: preference
importance: high
source_from_seq: 12
source_until_seq: 18

The user prefers concise proposals.
"#;
    write_memory_file(workspace.path(), "2026-07-17", current);
    let store = test_store(workspace.path()).await;
    let updated = current.replace("occurred_at: 1784251800", "occurred_at: 1784251801");

    let error = store
        .validate_source_edit(
            "user-1",
            "memory/2026-07-17.md",
            Some(current),
            &updated,
            UTC,
        )
        .await
        .expect_err("generated provenance must be immutable");

    assert!(matches!(error, talos_error::TalosError::Conflict(_)));
}

#[tokio::test]
async fn search_and_get_ten_thousand_entries() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut content = String::new();
    for index in 0..10_000_u64 {
        content.push_str(&format!(
            "## [09:30] Scale memory {index}\n\
memory_id: mem_{index:032x}\n\
occurred_at: {}\n\
session: scale-session\n\
kind: fact\n\
importance: normal\n\
source_from_seq: {}\n\
source_until_seq: {}\n\n\
Scale benchmark memory {index}.\n\n",
            1_784_251_800_u64 + index,
            index + 1,
            index + 1,
        ));
    }
    write_memory_file(workspace.path(), "2026-07-17", &content);
    let store = test_store(workspace.path()).await;
    store
        .rebuild_scope("user-1", workspace.path(), "memory", UTC)
        .await
        .expect("rebuild scale fixture");
    let query = MemorySearchQuery {
        text: "scale benchmark".to_string(),
        limit: 20,
        kinds: None,
        since_at: None,
        until_at: None,
    };
    let memory_id = "mem_0000000000000000000000000000270f";
    let mut search_micros = Vec::with_capacity(100);
    let mut get_micros = Vec::with_capacity(100);

    for _ in 0..100 {
        let started = Instant::now();
        let results = store.search("user-1", &query).await.expect("scale search");
        search_micros.push(started.elapsed().as_micros());
        assert_eq!(results.len(), 20);

        let started = Instant::now();
        let memory = store.get("user-1", memory_id).await.expect("scale get");
        get_micros.push(started.elapsed().as_micros());
        assert_eq!(memory.expect("scale memory").memory_id, memory_id);
    }

    search_micros.sort_unstable();
    get_micros.sort_unstable();
    eprintln!(
        "memory benchmark: entries=10000 search_p95_us={} get_p95_us={}",
        search_micros[94], get_micros[94]
    );
}
