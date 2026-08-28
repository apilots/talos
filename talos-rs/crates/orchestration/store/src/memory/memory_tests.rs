use std::fs;

use chrono::{TimeZone, Utc};
use chrono_tz::UTC;

use super::bundle::current_memory_date;
use super::{
    DreamMemorySourceReplace, MemoryImportance, MemoryKind, MemorySearchQuery, MemoryStore,
};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDayRevision, DreamMemoryDaySnapshot, DreamMemoryDocument,
    DreamMemoryDocumentContext, DreamMemoryEntry, DreamMemoryProvenance, DreamMemorySourceRange,
    DreamMemoryWriteRequest, MemoryAgentId, MemoryCandidate, MemoryId, MemoryScope,
    MemorySessionId, MemoryTimezone, MemoryUserId, MemoryWriteBatch,
};

async fn test_store(root: &std::path::Path) -> MemoryStore {
    let database = root.join("memory-index.sqlite");
    MemoryStore::open(&database).await.expect("memory store")
}

fn memory_scope(root: &std::path::Path, user_id: &str) -> MemoryScope {
    memory_scope_for_agent(root, "agent-1", user_id)
}

fn memory_scope_for_agent(root: &std::path::Path, agent_id: &str, user_id: &str) -> MemoryScope {
    MemoryScope {
        agent_id: agent_id.to_string(),
        user_id: user_id.to_string(),
        workspace_root: root.to_path_buf(),
    }
}

fn write_memory_file(root: &std::path::Path, date: &str, content: &str) {
    let memory_dir = root.join("memory");
    fs::create_dir_all(&memory_dir).expect("memory directory");
    fs::write(memory_dir.join(format!("{date}.md")), content).expect("memory file");
}

fn dream_write_request(
    root: &std::path::Path,
    batch: &MemoryWriteBatch,
) -> DreamMemoryWriteRequest {
    let date = Utc
        .timestamp_opt(batch.occurred_at, 0)
        .single()
        .expect("Dream Memory timestamp")
        .format("%Y-%m-%d")
        .to_string();
    let date = DreamMemoryDate::parse(&date).expect("Dream Memory date");
    let content = fs::read_to_string(root.join("memory").join(format!("{date}.md"))).ok();
    DreamMemoryWriteRequest::from_snapshot(
        &DreamMemoryDaySnapshot::new(
            date,
            DreamMemoryDayRevision::from_content(content.as_deref()),
            Vec::new(),
        ),
        batch.clone(),
    )
}

#[derive(Clone, Copy)]
enum TestMemoryProvenance<'a> {
    UserAuthored,
    Generated {
        session_id: &'a str,
        from_seq: u64,
        until_seq: u64,
    },
}

#[derive(Clone, Copy)]
struct TestMemoryEntry<'a> {
    memory_id: &'a str,
    title: &'a str,
    body: &'a str,
    occurred_at: i64,
    kind: MemoryKind,
    importance: MemoryImportance,
    provenance: TestMemoryProvenance<'a>,
}

fn canonical_memory(date: &str, entries: &[TestMemoryEntry<'_>]) -> String {
    canonical_memory_for_agent("agent-1", date, entries)
}

fn canonical_memory_for_agent(
    agent_id: &str,
    date: &str,
    entries: &[TestMemoryEntry<'_>],
) -> String {
    let context = DreamMemoryDocumentContext::new(
        MemoryAgentId::parse(agent_id).expect("agent id"),
        MemoryUserId::parse("user-1").expect("user id"),
        DreamMemoryDate::parse(date).expect("memory date"),
        MemoryTimezone::new(UTC),
    );
    let generated_at = entries
        .first()
        .expect("canonical memory fixture requires one entry")
        .occurred_at;
    let mut document =
        DreamMemoryDocument::new(context, generated_at).expect("Dream Memory document");
    for entry in entries {
        let provenance = match entry.provenance {
            TestMemoryProvenance::UserAuthored => DreamMemoryProvenance::UserAuthored,
            TestMemoryProvenance::Generated {
                session_id,
                from_seq,
                until_seq,
            } => DreamMemoryProvenance::Generated {
                session_id: MemorySessionId::parse(session_id).expect("session id"),
                source_range: DreamMemorySourceRange::new(from_seq, until_seq)
                    .expect("source range"),
            },
        };
        document
            .create_entry(
                DreamMemoryEntry::builder()
                    .memory_id(MemoryId::parse(entry.memory_id).expect("memory id"))
                    .title(entry.title)
                    .body(entry.body)
                    .occurred_at(entry.occurred_at)
                    .kind(entry.kind)
                    .importance(entry.importance)
                    .provenance(provenance)
                    .build()
                    .expect("memory entry"),
            )
            .expect("create memory entry");
    }
    document.render().expect("render Dream Memory")
}

fn test_entry<'a>(memory_id: &'a str, title: &'a str, body: &'a str) -> TestMemoryEntry<'a> {
    TestMemoryEntry {
        memory_id,
        title,
        body,
        occurred_at: 1_784_251_800,
        kind: MemoryKind::Fact,
        importance: MemoryImportance::Normal,
        provenance: TestMemoryProvenance::UserAuthored,
    }
}

#[tokio::test]
async fn rebuild_rejects_source_sequences_outside_sqlite_integer_bounds() {
    let workspace = tempfile::tempdir().expect("workspace");
    let content = canonical_memory(
        "2026-07-17",
        &[TestMemoryEntry {
            provenance: TestMemoryProvenance::Generated {
                session_id: "session-1",
                from_seq: i64::MAX as u64 + 1,
                until_seq: i64::MAX as u64 + 1,
            },
            ..test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Invalid range",
                "Invalid range.",
            )
        }],
    );
    write_memory_file(workspace.path(), "2026-07-17", &content);
    let store = test_store(workspace.path()).await;

    let error = store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect_err("out-of-range sequence must fail");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
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
    let content = canonical_memory(
        "2026-07-17",
        &[
            TestMemoryEntry {
                kind: MemoryKind::Preference,
                importance: MemoryImportance::High,
                provenance: TestMemoryProvenance::Generated {
                    session_id: "session-1",
                    from_seq: 12,
                    until_seq: 18,
                },
                ..test_entry(
                    "mem_0123456789abcdef0123456789abcdef",
                    "Durable preference",
                    "The user prefers concise architecture proposals.",
                )
            },
            test_entry(
                "mem_11111111111111111111111111111111",
                "Existing note",
                "SQLite can be rebuilt from Markdown.",
            ),
            TestMemoryEntry {
                occurred_at: 1_784_246_400,
                ..test_entry(
                    "mem_22222222222222222222222222222222",
                    "Untimed note",
                    "Midnight default memory.",
                )
            },
        ],
    );
    write_memory_file(workspace.path(), "2026-07-17", &content);
    let store = test_store(workspace.path()).await;

    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("first rebuild");
    let first = store
        .search(
            &memory_scope(workspace.path(), "user-1"),
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
            .get(
                &memory_scope(workspace.path(), "user-1"),
                first[0].memory.memory_id.as_str()
            )
            .await
            .expect("get memory"),
        Some(first[0].memory.clone())
    );

    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("repeat rebuild");
    let rebuild_query = MemorySearchQuery {
        text: "SQLite".to_string(),
        limit: 6,
        kinds: None,
        since_at: None,
        until_at: None,
    };
    let rebuilt_id = store
        .search(&memory_scope(workspace.path(), "user-1"), &rebuild_query)
        .await
        .expect("rebuild search")[0]
        .memory
        .memory_id
        .clone();
    let untimed = store
        .search(
            &memory_scope(workspace.path(), "user-1"),
            &MemorySearchQuery {
                text: "Midnight default".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("user-authored search");
    assert_eq!(
        untimed[0].memory.provenance,
        DreamMemoryProvenance::UserAuthored
    );
    assert_eq!(untimed[0].memory.occurred_at, 1_784_246_400);
    assert_eq!(
        fs::read_to_string(workspace.path().join("memory/2026-07-17.md")).expect("read source"),
        content
    );

    drop(store);
    fs::remove_file(workspace.path().join("memory-index.sqlite")).expect("remove derived index");
    let rebuilt_store = test_store(workspace.path()).await;
    rebuilt_store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("rebuild deleted index");
    assert_eq!(
        rebuilt_store
            .search(&memory_scope(workspace.path(), "user-1"), &rebuild_query)
            .await
            .expect("search rebuilt index")[0]
            .memory
            .memory_id,
        rebuilt_id
    );
}

#[tokio::test]
async fn rebuild_uses_the_configured_memory_directory() {
    let workspace = tempfile::tempdir().expect("workspace");
    let memory_dir = workspace.path().join("journal");
    fs::create_dir_all(&memory_dir).expect("memory directory");
    fs::write(
        memory_dir.join("2026-07-17.md"),
        canonical_memory(
            "2026-07-17",
            &[test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Configured source",
                "The configured directory is canonical.",
            )],
        ),
    )
    .expect("memory file");
    let store = test_store(workspace.path()).await;

    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "journal", UTC)
        .await
        .expect("rebuild configured directory");

    assert_eq!(
        store
            .search(
                &memory_scope(workspace.path(), "user-1"),
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
        &canonical_memory(
            "2026-07-17",
            &[test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Long note",
                &body,
            )],
        ),
    );
    let store = test_store(workspace.path()).await;
    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("rebuild");

    let result = store
        .search(
            &memory_scope(workspace.path(), "user-1"),
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
async fn rebuild_replaces_changed_sources_and_isolates_agent_user_workspace_scopes() {
    let workspace = tempfile::tempdir().expect("workspace");
    let other_workspace = tempfile::tempdir().expect("other workspace");
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        &canonical_memory(
            "2026-07-17",
            &[test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "First note",
                "alpha memory",
            )],
        ),
    );
    let store = test_store(workspace.path()).await;
    let other_agent_scope = memory_scope_for_agent(workspace.path(), "agent-2", "user-1");
    let other_workspace_scope = memory_scope(other_workspace.path(), "user-1");
    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("user one rebuild");
    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-2"), "memory", UTC)
        .await
        .expect("user two rebuild");
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        &canonical_memory_for_agent(
            "agent-2",
            "2026-07-17",
            &[test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "First note",
                "alpha memory",
            )],
        ),
    );
    store
        .rebuild_scope(&other_agent_scope, "memory", UTC)
        .await
        .expect("other Agent rebuild");
    write_memory_file(
        other_workspace.path(),
        "2026-07-17",
        &canonical_memory(
            "2026-07-17",
            &[test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Workspace note",
                "gamma memory",
            )],
        ),
    );
    store
        .rebuild_scope(&other_workspace_scope, "memory", UTC)
        .await
        .expect("other workspace rebuild");

    write_memory_file(
        workspace.path(),
        "2026-07-17",
        &canonical_memory(
            "2026-07-17",
            &[test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Second note",
                "beta memory",
            )],
        ),
    );
    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
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
        .search(&memory_scope(workspace.path(), "user-1"), &search("alpha"))
        .await
        .expect("removed source search")
        .is_empty());
    assert_eq!(
        store
            .search(&memory_scope(workspace.path(), "user-1"), &search("beta"))
            .await
            .expect("changed source search")
            .len(),
        1
    );
    assert_eq!(
        store
            .search(&other_agent_scope, &search("alpha"))
            .await
            .expect("other Agent search")
            .len(),
        1
    );
    assert_eq!(
        store
            .search(&other_workspace_scope, &search("gamma"))
            .await
            .expect("other workspace search")
            .len(),
        1
    );

    fs::remove_file(workspace.path().join("memory/2026-07-17.md")).expect("remove memory source");
    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("deleted source rebuild");
    assert!(store
        .search(&memory_scope(workspace.path(), "user-1"), &search("beta"))
        .await
        .expect("deleted source search")
        .is_empty());
    assert_eq!(
        store
            .search(&memory_scope(workspace.path(), "user-2"), &search("alpha"))
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
    let scope = memory_scope(workspace.path(), "user-1");
    assert!(store
        .list_scope_dates(&scope, "memory", UTC)
        .await
        .expect("empty bundle navigation")
        .is_empty());
    assert!(workspace.path().join("memory/index.md").exists());
    assert!(!workspace.path().join("memory/log.md").exists());
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

    let first_request = dream_write_request(workspace.path(), &batch);
    let retry_request = dream_write_request(workspace.path(), &batch);
    let (first_write, retry_write) = tokio::join!(
        store.write_batch(&scope, "memory", UTC, &first_request),
        store.write_batch(&scope, "memory", UTC, &retry_request),
    );
    assert_eq!(
        usize::from(first_write.is_ok()) + usize::from(retry_write.is_ok()),
        1
    );
    let conflict = first_write
        .err()
        .or_else(|| retry_write.err())
        .expect("stale write");
    assert!(conflict.is_code(talos_error::ErrorCode::Conflict));
    store
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &batch),
        )
        .await
        .expect("fresh idempotent retry");

    let results = store
        .search(
            &memory_scope(workspace.path(), "user-1"),
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
    assert!(matches!(
        results[0].memory.provenance,
        DreamMemoryProvenance::Generated {
            source_range,
            ..
        } if source_range == DreamMemorySourceRange::new(12, 18).expect("source range")
    ));
    let memory_dir = workspace.path().join("memory");
    let daily = fs::read_to_string(memory_dir.join("2026-07-17.md")).expect("read written memory");
    assert_eq!(daily.matches("talos:dream-memory:entry").count(), 1);
    assert!(daily.contains("generated:"));
    assert!(daily.contains("by: talos-agent/agent-1"));
    let index = fs::read_to_string(memory_dir.join("index.md")).expect("read bundle index");
    assert_eq!(
        talos_memory::DreamMemoryBundle::parse_index_dates(&index).expect("parse bundle index"),
        vec![DreamMemoryDate::parse("2026-07-17").expect("date")]
    );
    assert!(index.contains("Contains 1 durable memory: Durable preference."));
    let log = fs::read_to_string(memory_dir.join("log.md")).expect("read bundle log");
    assert_eq!(log.matches(" revision `").count(), 1);
    assert!(log.contains("Contains 1 durable memory: Durable preference."));
    assert!(log.contains(&format!(
        "## {}",
        current_memory_date(UTC).expect("current log date")
    )));

    drop(store);
    fs::remove_file(workspace.path().join("memory-index.sqlite")).expect("remove derived index");
    let recovered_store = test_store(workspace.path()).await;
    let mut regenerated_batch = batch.clone();
    regenerated_batch.candidates[0].body = "A different regenerated memory.".to_string();
    recovered_store
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &regenerated_batch),
        )
        .await
        .expect("retry uses canonical Markdown after the index sync fails");
    assert_eq!(
        recovered_store
            .search(
                &memory_scope(workspace.path(), "user-1"),
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
            &memory_scope(workspace.path(), "user-1"),
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
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &batch),
        )
        .await
        .expect("write canonical Markdown");

    drop(store);
    fs::remove_file(workspace.path().join("memory-index.sqlite")).expect("remove derived index");
    let recovered_store = test_store(workspace.path()).await;
    let mut empty_retry = batch;
    empty_retry.candidates.clear();
    recovered_store
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &empty_retry),
        )
        .await
        .expect("empty retry syncs canonical Markdown");

    assert_eq!(
        recovered_store
            .search(
                &memory_scope(workspace.path(), "user-1"),
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
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &first),
        )
        .await
        .expect("write first range");
    let path = workspace.path().join("memory/2026-07-17.md");
    let original = fs::read_to_string(&path).expect("read first range");
    let overlapping = memory_batch("session-1", 1_784_251_800, 20, 30, "Overlapping range.");

    let error = store
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &overlapping),
        )
        .await
        .expect_err("partial overlap must fail");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        fs::read_to_string(path).expect("read unchanged source"),
        original
    );
}

#[tokio::test]
async fn concurrent_cross_date_writes_preserve_scope_uniqueness() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let first = memory_batch("session-1", 1_784_251_800, 10, 20, "Canonical record.");
    let second = memory_batch("session-1", 1_784_338_200, 10, 20, "Canonical record.");

    let first_request = dream_write_request(workspace.path(), &first);
    let second_request = dream_write_request(workspace.path(), &second);
    let scope = memory_scope(workspace.path(), "user-1");
    let (first_result, second_result) = tokio::join!(
        store.write_batch(&scope, "memory", UTC, &first_request),
        store.write_batch(&scope, "memory", UTC, &second_request),
    );

    assert_eq!(
        usize::from(first_result.is_ok()) + usize::from(second_result.is_ok()),
        1
    );
    for error in [first_result.err(), second_result.err()]
        .into_iter()
        .flatten()
    {
        assert!(error.is_code(talos_error::ErrorCode::Conflict));
    }
    assert_eq!(
        [
            workspace.path().join("memory/2026-07-17.md"),
            workspace.path().join("memory/2026-07-18.md"),
        ]
        .into_iter()
        .filter(|path| path.exists())
        .count(),
        1
    );
}

#[tokio::test]
async fn write_batch_rejects_an_invalid_source_in_the_same_scope() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    let first = memory_batch("session-1", 1_784_251_800, 1, 2, "First source record.");
    store
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &first),
        )
        .await
        .expect("write first record");
    write_memory_file(
        workspace.path(),
        "2026-07-18",
        "## Invalid\nmemory_id: invalid\n\nMalformed unrelated source.\n",
    );
    let second = memory_batch("session-1", 1_784_251_800, 3, 4, "Second source record.");

    let error = store
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &second),
        )
        .await
        .expect_err("scope validation must reject an invalid source");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn search_applies_filters_weight_and_stable_rank() {
    let workspace = tempfile::tempdir().expect("workspace");
    let content = canonical_memory(
        "2026-07-17",
        &[
            TestMemoryEntry {
                kind: MemoryKind::Preference,
                importance: MemoryImportance::High,
                provenance: TestMemoryProvenance::Generated {
                    session_id: "session-1",
                    from_seq: 1,
                    until_seq: 1,
                },
                ..test_entry(
                    "mem_11111111111111111111111111111111",
                    "Architecture preference",
                    "The user prefers concise architecture. 中文架构记忆。 The user said \"concise\".",
                )
            },
            TestMemoryEntry {
                kind: MemoryKind::Episode,
                importance: MemoryImportance::Low,
                provenance: TestMemoryProvenance::Generated {
                    session_id: "session-2",
                    from_seq: 2,
                    until_seq: 2,
                },
                ..test_entry(
                    "mem_22222222222222222222222222222222",
                    "Architecture episode",
                    "The user reviewed concise architecture.",
                )
            },
        ],
    );
    write_memory_file(workspace.path(), "2026-07-17", &content);
    let store = test_store(workspace.path()).await;
    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("rebuild");

    let ranked = store
        .search(
            &memory_scope(workspace.path(), "user-1"),
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
            &memory_scope(workspace.path(), "user-1"),
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
            &memory_scope(workspace.path(), "user-1"),
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
async fn rebuild_rejects_noncanonical_plain_headings() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        "## Audit scope\n\n- Preserve existing Markdown memory\n",
    );
    let store = test_store(workspace.path()).await;

    let error = store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect_err("runtime must reject noncanonical Markdown");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn corrupt_index_is_recreated_from_markdown() {
    let workspace = tempfile::tempdir().expect("workspace");
    write_memory_file(
        workspace.path(),
        "2026-07-17",
        &canonical_memory(
            "2026-07-17",
            &[test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Recoverable note",
                "canonical markdown survives",
            )],
        ),
    );
    fs::write(workspace.path().join("memory/log.md"), "invalid log").expect("corrupt derived log");
    let database = workspace.path().join("memory-index.sqlite");
    fs::write(&database, b"not a sqlite database").expect("corrupt index");

    let store = MemoryStore::open(&database)
        .await
        .expect("corrupt index should be replaced");
    store
        .rebuild_scope(&memory_scope(workspace.path(), "user-1"), "memory", UTC)
        .await
        .expect("rebuild from markdown");

    assert_eq!(
        store
            .search(
                &memory_scope(workspace.path(), "user-1"),
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
    let rebuilt_log =
        fs::read_to_string(workspace.path().join("memory/log.md")).expect("read rebuilt log");
    assert!(rebuilt_log.contains("**Recovery**:"));
}

#[tokio::test]
async fn source_edit_keeps_generated_provenance_immutable() {
    let workspace = tempfile::tempdir().expect("workspace");
    let current = canonical_memory(
        "2026-07-17",
        &[TestMemoryEntry {
            kind: MemoryKind::Preference,
            importance: MemoryImportance::High,
            provenance: TestMemoryProvenance::Generated {
                session_id: "session-1",
                from_seq: 12,
                until_seq: 18,
            },
            ..test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Durable preference",
                "The user prefers concise proposals.",
            )
        }],
    );
    write_memory_file(workspace.path(), "2026-07-17", &current);
    let store = test_store(workspace.path()).await;
    let updated = current.replace(
        "- **Occurred At:** 1784251800",
        "- **Occurred At:** 1784251801",
    );
    let scope = memory_scope(workspace.path(), "user-1");

    let error = store
        .replace_source(DreamMemorySourceReplace {
            scope: &scope,
            memory_dirname: "memory",
            source_date: DreamMemoryDate::parse("2026-07-17").expect("source date"),
            expected_content: Some(&current),
            updated: &updated,
            timezone: UTC,
        })
        .await
        .expect_err("generated provenance must be immutable");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
}

#[tokio::test]
async fn same_content_source_replace_preserves_scope_and_index_invariants() {
    let workspace = tempfile::tempdir().expect("workspace");
    let content = canonical_memory(
        "2026-07-17",
        &[test_entry(
            "mem_0123456789abcdef0123456789abcdef",
            "Canonical source",
            "Same-content replace must synchronize this source.",
        )],
    );
    write_memory_file(workspace.path(), "2026-07-17", &content);
    let store = test_store(workspace.path()).await;
    let scope = memory_scope(workspace.path(), "user-1");
    let source_date = DreamMemoryDate::parse("2026-07-17").expect("source date");

    store
        .replace_source(DreamMemorySourceReplace {
            scope: &scope,
            memory_dirname: "memory",
            source_date,
            expected_content: Some(&content),
            updated: &content,
            timezone: UTC,
        })
        .await
        .expect("same-content replace synchronizes source");

    assert_eq!(
        store
            .search(
                &memory_scope(workspace.path(), "user-1"),
                &MemorySearchQuery {
                    text: "synchronize this source".to_string(),
                    limit: 1,
                    kinds: None,
                    since_at: None,
                    until_at: None,
                },
            )
            .await
            .expect("search synchronized source")
            .len(),
        1
    );

    let duplicate = canonical_memory(
        "2026-07-18",
        &[TestMemoryEntry {
            occurred_at: 1_784_338_200,
            ..test_entry(
                "mem_0123456789abcdef0123456789abcdef",
                "Duplicate source",
                "The same id must remain unique across dates.",
            )
        }],
    );
    write_memory_file(workspace.path(), "2026-07-18", &duplicate);
    let conflict = store
        .replace_source(DreamMemorySourceReplace {
            scope: &scope,
            memory_dirname: "memory",
            source_date,
            expected_content: Some(&content),
            updated: &content,
            timezone: UTC,
        })
        .await
        .expect_err("same-content replace must validate the complete scope");
    assert!(conflict.is_code(talos_error::ErrorCode::Conflict));

    fs::remove_file(workspace.path().join("memory/2026-07-18.md"))
        .expect("remove conflicting source");
    store.pool.close().await;
    store
        .replace_source(DreamMemorySourceReplace {
            scope: &scope,
            memory_dirname: "memory",
            source_date,
            expected_content: Some(&content),
            updated: &content,
            timezone: UTC,
        })
        .await
        .expect_err("failed same-content index sync must retain stale marker");
    assert!(workspace.path().join(".dream-memory-index-stale").exists());

    let recovered = test_store(workspace.path()).await;
    let records = recovered
        .search_scope(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &MemorySearchQuery {
                text: "synchronize this source".to_string(),
                limit: 1,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("read boundary rebuilds stale same-content index");
    assert_eq!(records.len(), 1);
    let memory_dir = workspace.path().join("memory");
    assert!(memory_dir.join("index.md").exists());
    assert!(memory_dir.join("log.md").exists());
    assert_eq!(
        fs::read_to_string(memory_dir.join("log.md"))
            .expect("read recovered bundle log")
            .matches(" revision `")
            .count(),
        1
    );
    assert!(!workspace.path().join(".dream-memory-index-stale").exists());
}

#[tokio::test]
async fn committed_markdown_recovers_a_stale_index_on_the_next_read_boundary() {
    let workspace = tempfile::tempdir().expect("workspace");
    let store = test_store(workspace.path()).await;
    store.pool.close().await;
    let batch = memory_batch("session-1", 1_784_251_800, 12, 18, "recover stale index");

    store
        .write_batch(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &dream_write_request(workspace.path(), &batch),
        )
        .await
        .expect_err("closed index must fail after the Markdown commit");
    assert!(workspace.path().join(".dream-memory-index-stale").exists());

    let recovered = test_store(workspace.path()).await;
    let records = recovered
        .search_scope(
            &memory_scope(workspace.path(), "user-1"),
            "memory",
            UTC,
            &MemorySearchQuery {
                text: "recover stale index".to_string(),
                limit: 6,
                kinds: None,
                since_at: None,
                until_at: None,
            },
        )
        .await
        .expect("search recovered index");

    assert_eq!(records.len(), 1);
    assert!(!workspace.path().join(".dream-memory-index-stale").exists());
}
