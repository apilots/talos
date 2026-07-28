use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use crate::project::{ProjectStore, StorageProjectStore};
use serde_json::Value;
use talos_storage::{Row, SqliteStorage, Storage};
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TokenUsage};
use talos_types::{AgentTurnRef, ModelRef, ModelSelection, SessionConf, DEFAULT_SESSION_TITLE};

fn sample_context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("group-1".to_string()),
        thread_id: Some("topic-1".to_string()),
        project_root: None,
        environment: None,
    }
}

fn record(
    id: &str,
    user_id: &str,
    project_id: Option<String>,
    current_directory: Option<PathBuf>,
) -> SessionRecord {
    let context = TalosContext {
        user_id: user_id.to_string(),
        ..sample_context()
    };
    SessionRecord {
        id: id.to_string(),
        agent_id: "agent-a".to_string(),
        tree_identity: SessionTreeIdentity::Primary,
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: user_id.to_string(),
        context,
        model: "gpt-5.3".to_string(),
        created_at: 1,
        last_activity: 1,
        message_count: 0,
        latest_turn_seq: 0,
        memory_processed_until_seq: 0,
        usage: TokenUsage::default(),
        conf: SessionConf::default(),
        workspace_id: Some("workspace-alpha".to_string()),
        project_id,
        current_directory,
    }
}

async fn store() -> DatabaseSessionStore {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    DatabaseSessionStore::new(storage).await.unwrap()
}

#[test]
fn test_normalized_optional_string_treats_blank_as_none() {
    assert_eq!(DatabaseSessionStore::normalized_optional_string(None), None);
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some(String::new())),
        None
    );
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some("   ".to_string())),
        None
    );
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some(" project-1 ".to_string())),
        Some("project-1".to_string())
    );
}

#[tokio::test]
async fn test_insert_and_get_session_record_roundtrip() {
    let store = store().await;
    let mut record = record(
        "session-1",
        "user-1",
        Some("project-1".to_string()),
        Some(PathBuf::from("/repo")),
    );
    record.conf = SessionConf {
        model_selection: Some(ModelSelection {
            model_ref: ModelRef {
                provider: "deepseek".to_string(),
                id: "deepseek-v4-flash".to_string(),
            },
            reasoning_effort: None,
        }),
    };

    store.insert_session_record(&record, "api").await.unwrap();
    let loaded = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(loaded, record);
}

#[tokio::test]
async fn subagent_identity_roundtrips_and_rejects_duplicate_tree_path() {
    let store = store().await;
    let parent = AgentTurnRef {
        session_id: "root-session".to_string(),
        turn_id: "root-turn".to_string(),
    };
    let mut child = record("child-1", "user-1", None, None);
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: parent.clone(),
        tree_root_session_id: "root-session".to_string(),
        path: "/root/reviewer".parse().unwrap(),
    };
    store.insert_session_record(&child, "api").await.unwrap();

    let loaded = store.get_session_record("child-1").await.unwrap().unwrap();
    assert_eq!(loaded, child);

    let mut duplicate = record("child-2", "user-1", None, None);
    duplicate.tree_identity = SessionTreeIdentity::Subagent {
        parent,
        tree_root_session_id: "root-session".to_string(),
        path: "/root/reviewer".parse().unwrap(),
    };
    assert!(store
        .insert_session_record(&duplicate, "api")
        .await
        .is_err());
}

#[tokio::test]
async fn active_subagent_turn_check_uses_one_tree_query() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let mut child = record("child-session", "user-1", None, None);
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: AgentTurnRef {
            session_id: "root-session".to_string(),
            turn_id: "root-turn".to_string(),
        },
        tree_root_session_id: "root-session".to_string(),
        path: "/root/reviewer".parse().unwrap(),
    };
    store.insert_session_record(&child, "api").await.unwrap();
    storage
        .execute(
            "INSERT INTO active_turns (session_id, turn_id, status, updated_at) VALUES (?, 'turn-1', 'running', 1)",
            &[&child.id],
        )
        .await
        .unwrap();

    assert!(store
        .has_active_subagent_turn("root-session")
        .await
        .unwrap());
    assert!(!store.has_active_subagent_turn("other-root").await.unwrap());
}

#[tokio::test]
async fn delete_session_tree_removes_descendants_and_runtime_rows() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let root = record("root-session", "user-1", None, None);
    let mut child = record("child-session", "user-1", None, None);
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: AgentTurnRef {
            session_id: root.id.clone(),
            turn_id: "root-turn".to_string(),
        },
        tree_root_session_id: root.id.clone(),
        path: "/root/reviewer".parse().unwrap(),
    };
    store.insert_session_record(&root, "api").await.unwrap();
    store.insert_session_record(&child, "api").await.unwrap();
    for session_id in [&root.id, &child.id] {
        storage
            .execute(
                "INSERT INTO timeline_events (session_id, turn_id, message_id, sequence, event_type, payload_json, created_at_ms) VALUES (?, 'turn-1', 'message-1', 1, 'turn_started', '{}', 1)",
                &[session_id],
            )
            .await
            .unwrap();
        storage
            .execute(
                "INSERT INTO active_turns (session_id, turn_id, status, updated_at) VALUES (?, 'turn-1', 'running', 1)",
                &[session_id],
            )
            .await
            .unwrap();
        storage
            .execute(
                "INSERT INTO message_idempotency_keys (session_id, idempotency_key, request_hash, turn_id, status, created_at, updated_at) VALUES (?, 'key-1', 'hash-1', 'turn-1', 'pending', 1, 1)",
                &[session_id],
            )
            .await
            .unwrap();
    }

    assert!(store.delete_session_tree(&root.id).await.unwrap());

    let counts = storage
        .query(
            "SELECT (SELECT COUNT(*) FROM sessions) AS sessions, (SELECT COUNT(*) FROM timeline_events) AS events, (SELECT COUNT(*) FROM active_turns) AS active_turns, (SELECT COUNT(*) FROM message_idempotency_keys) AS idempotency_keys",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        (
            counts[0].get_i64("sessions"),
            counts[0].get_i64("events"),
            counts[0].get_i64("active_turns"),
            counts[0].get_i64("idempotency_keys"),
        ),
        (Some(0), Some(0), Some(0), Some(0))
    );
}

#[tokio::test]
async fn test_get_session_record_reports_invalid_conf_json() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let record = record("session-1", "user-1", None, None);
    store.insert_session_record(&record, "api").await.unwrap();
    storage
        .update(
            "sessions",
            &Row::new(
                vec!["conf_json".to_string()],
                vec![Value::String("{invalid".to_string())],
            ),
            "session_key = ?",
            &["session-1"],
        )
        .await
        .unwrap();

    let error = store
        .get_session_record("session-1")
        .await
        .expect_err("invalid session conf should fail");

    assert!(error
        .to_string()
        .contains("parse session 'session-1' conf_json"));
}

#[tokio::test]
async fn test_update_session_record_persists_fields() {
    let store = store().await;
    let mut record = record("session-1", "user-1", None, None);
    store.insert_session_record(&record, "api").await.unwrap();
    store
        .advance_memory_processed_until_seq("session-1", 0, 3)
        .await
        .unwrap();

    record.title = "Custom Title".to_string();
    record.message_count = 2;
    record.usage = TokenUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        ..TokenUsage::default()
    };
    record.model = "gpt-5.4".to_string();
    assert!(store.update_session_record(&record).await.unwrap());

    let loaded = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.title, "Custom Title");
    assert_eq!(loaded.message_count, 2);
    assert_eq!(loaded.memory_processed_until_seq, 3);
    assert_eq!(loaded.latest_turn_seq, 3);
    assert_eq!(loaded.usage.total_tokens, 15);
    assert_eq!(loaded.model, "gpt-5.4");
}

#[tokio::test]
async fn memory_cursor_advance_is_idempotent() {
    let store = store().await;
    store
        .insert_session_record(&record("session-1", "user-1", None, None), "api")
        .await
        .unwrap();

    store
        .advance_memory_processed_until_seq("session-1", 0, 4)
        .await
        .unwrap();
    store
        .advance_memory_processed_until_seq("session-1", 0, 4)
        .await
        .unwrap();

    assert_eq!(
        store
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap()
            .memory_processed_until_seq,
        4
    );
}

#[tokio::test]
async fn memory_cursor_rejects_non_contiguous_advance() {
    let store = store().await;
    store
        .insert_session_record(&record("session-1", "user-1", None, None), "api")
        .await
        .unwrap();

    let error = store
        .advance_memory_processed_until_seq("session-1", 1, 4)
        .await
        .expect_err("stale cursor should fail");

    assert!(matches!(error, talos_error::TalosError::Conflict(_)));
}

#[tokio::test]
async fn test_delete_session_record_removes_session() {
    let store = store().await;
    let record = record("session-1", "user-1", None, None);
    store.insert_session_record(&record, "api").await.unwrap();

    assert!(store.delete_session_record("session-1").await.unwrap());
    assert!(store
        .get_session_record("session-1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_list_session_records_filters_by_project_id() {
    let store = store().await;
    store
        .insert_session_record(
            &record("session-1", "user-1", Some("project-1".to_string()), None),
            "api",
        )
        .await
        .unwrap();
    store
        .insert_session_record(
            &record("session-2", "user-1", Some("project-2".to_string()), None),
            "api",
        )
        .await
        .unwrap();

    let records = store
        .list_session_records(&SessionQuery {
            project_id: Some("project-1".to_string()),
            ..SessionQuery::default()
        })
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "session-1");
}

#[tokio::test]
async fn test_list_session_records_respects_limit_and_offset() {
    let store = store().await;
    store
        .insert_session_record(&record("session-1", "user-1", None, None), "api")
        .await
        .unwrap();
    store
        .insert_session_record(&record("session-2", "user-1", None, None), "api")
        .await
        .unwrap();
    store
        .insert_session_record(&record("session-3", "user-1", None, None), "api")
        .await
        .unwrap();

    let records = store
        .list_session_records(&SessionQuery {
            limit: Some(1),
            offset: Some(1),
            ..SessionQuery::default()
        })
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
}

#[tokio::test]
async fn memory_pending_session_pages_use_a_stable_cursor() {
    let store = store().await;
    let mut processed = record("session-1", "user-1", None, None);
    processed.last_activity = 3;
    processed.message_count = 2;
    processed.latest_turn_seq = 1;
    processed.memory_processed_until_seq = 1;
    let mut first_pending = record("session-2", "user-1", None, None);
    first_pending.last_activity = 2;
    first_pending.message_count = 4;
    first_pending.latest_turn_seq = 2;
    first_pending.memory_processed_until_seq = 1;
    let mut second_pending = record("session-3", "user-1", None, None);
    second_pending.last_activity = 1;
    second_pending.message_count = 2;
    second_pending.latest_turn_seq = 1;
    for session in [&processed, &first_pending, &second_pending] {
        store
            .insert_session_record(session, "api")
            .await
            .expect("insert session");
    }

    let first_page = store
        .list_session_records(&SessionQuery {
            limit: Some(1),
            memory_pending_only: true,
            ..SessionQuery::default()
        })
        .await
        .expect("first pending page");
    let second_page = store
        .list_session_records(&SessionQuery {
            limit: Some(1),
            memory_pending_only: true,
            after: Some(SessionPageCursor {
                last_activity: first_page[0].last_activity,
                session_id: first_page[0].id.clone(),
            }),
            ..SessionQuery::default()
        })
        .await
        .expect("second pending page");

    assert_eq!(first_page[0].id, "session-2");
    assert_eq!(second_page[0].id, "session-3");
}

#[tokio::test]
async fn memory_pending_uses_monotonic_turn_seq_after_message_count_reset() {
    let store = store().await;
    let mut cleared = record("session-1", "user-1", None, None);
    cleared.message_count = 0;
    cleared.latest_turn_seq = 101;
    cleared.memory_processed_until_seq = 100;
    store
        .insert_session_record(&cleared, "api")
        .await
        .expect("insert cleared session");

    let records = store
        .list_session_records(&SessionQuery {
            memory_pending_only: true,
            ..SessionQuery::default()
        })
        .await
        .expect("list pending sessions");

    assert_eq!(
        records
            .into_iter()
            .map(|record| (record.id, record.latest_turn_seq))
            .collect::<Vec<_>>(),
        vec![("session-1".to_string(), 101)]
    );
}

#[tokio::test]
async fn test_list_projects_returns_session_count() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let project_store = StorageProjectStore::new(storage).await.unwrap();
    let project = project_store
        .create_project("repo", &PathBuf::from("/repo"))
        .await
        .unwrap();
    store
        .insert_session_record(
            &record(
                "session-1",
                "user-1",
                Some(project.project_id.clone()),
                None,
            ),
            "api",
        )
        .await
        .unwrap();

    let projects = project_store.list_projects().await.unwrap();

    assert_eq!(projects[0].session_count, 1);
}
