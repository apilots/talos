use sqlx::Row;
use std::sync::Arc;
use talos_storage::SqliteStorage;

use super::{run_migrations, test_migrations_dir};

#[tokio::test]
async fn migration_replaces_legacy_pending_actions_with_user_action_store() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    let pool = storage.pool().clone();
    sqlx::raw_sql(
        r#"
        CREATE TABLE active_turns (
            session_id TEXT PRIMARY KEY,
            turn_id TEXT NOT NULL,
            message_id TEXT,
            status TEXT NOT NULL,
            last_event_seq INTEGER,
            pending_user_action_json TEXT,
            updated_at INTEGER NOT NULL
        );
        INSERT INTO active_turns VALUES (
            'waiting-session', 'turn-1', 'message-1', 'waiting_user_action', 1,
            '{"type":"dynamic_tool_result"}', 10
        );
        INSERT INTO active_turns VALUES (
            'running-session', 'turn-2', 'message-2', 'running', 2, NULL, 11
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &test_migrations_dir()).await.unwrap();

    let columns = sqlx::query("PRAGMA table_info(active_turns)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(columns.contains("current_action_id"));
    assert!(!columns.contains("pending_user_action_json"));
    let active_sessions =
        sqlx::query_scalar::<_, String>("SELECT session_id FROM active_turns ORDER BY session_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(active_sessions, vec!["running-session".to_string()]);

    use crate::session::{DatabaseSessionStore, UserActionRecord, UserActionStore};
    use talos_types::{
        DynamicToolResultParams, PendingUserAction, UserActionParams, UserActionState,
    };
    let store = DatabaseSessionStore::new(storage).await.unwrap();
    let record = UserActionRecord {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        action: PendingUserAction {
            action_id: "action-1".to_string(),
            call_id: "call-1".to_string(),
            item_id: None,
            state: UserActionState::Pending,
            params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
                tool_name: "external-tool".to_string(),
            }),
        },
        response: None,
        version: 0,
        updated_at: 12,
    };
    assert_eq!(store.create(record.clone()).await.unwrap(), record);
    assert_eq!(
        store.get("session-1", "turn-1", "action-1").await.unwrap(),
        Some(record)
    );
}

#[tokio::test]
async fn migration_initializes_base_schema_once() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();

    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();

    let tables = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('sessions', 'bindings', 'projects', 'users', 'user_tokens', 'timeline_events', 'active_turns', 'message_idempotency_keys', 'user_actions') ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        vec![
            "active_turns".to_string(),
            "bindings".to_string(),
            "message_idempotency_keys".to_string(),
            "projects".to_string(),
            "sessions".to_string(),
            "timeline_events".to_string(),
            "user_actions".to_string(),
            "user_tokens".to_string(),
            "users".to_string(),
        ]
    );
}

#[tokio::test]
async fn migration_adopts_supported_unversioned_schema_without_losing_sessions() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    sqlx::raw_sql(
        r#"
        CREATE TABLE sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_key TEXT UNIQUE NOT NULL,
            user_id TEXT NOT NULL,
            channel TEXT NOT NULL,
            scope TEXT NOT NULL,
            peer_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            account_id TEXT,
            thread_id TEXT,
            created_at INTEGER NOT NULL,
            last_activity INTEGER NOT NULL,
            title TEXT NOT NULL DEFAULT 'Untitled Session',
            context_json TEXT,
            model TEXT,
            message_count INTEGER NOT NULL DEFAULT 0,
            usage_json TEXT,
            conf_json TEXT,
            project_id TEXT,
            current_directory TEXT,
            workspace_id TEXT,
            memory_processed_until_seq INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sessions (session_key, user_id, channel, scope, peer_id, agent_id, created_at, last_activity, message_count, memory_processed_until_seq) VALUES ('session-1', 'user-1', 'http', 'api', 'user-1', 'talos', 1, 1, 5, 4)",
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &test_migrations_dir()).await.unwrap();

    let session_id =
        sqlx::query_scalar::<_, String>("SELECT session_key FROM sessions WHERE session_key = ?")
            .bind("session-1")
            .fetch_one(&pool)
            .await
            .unwrap();
    let columns = sqlx::query("PRAGMA table_info(sessions)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<std::collections::BTreeSet<_>>();
    let latest_turn_seq =
        sqlx::query_scalar::<_, i64>("SELECT latest_turn_seq FROM sessions WHERE session_key = ?")
            .bind("session-1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (session_id, latest_turn_seq, columns.contains("agent_path")),
        ("session-1".to_string(), 4, true)
    );
}

#[tokio::test]
async fn migration_creates_profile_usage_schema() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();

    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'profile_turn_usage'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    let index_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_profile_turn_usage_finalized_completed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(index_count, 1);
}

#[tokio::test]
async fn migration_enforces_file_state_invariants() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO files (
            file_id, owner_user_id, project_id, storage_id, object_key,
            original_filename, mime_type, size_bytes, status, metadata_json,
            created_at, updated_at
        ) VALUES (?, ?, NULL, ?, ?, ?, NULL, NULL, 'uploading', '{}', ?, ?)
        "#,
    )
    .bind("file-uploading")
    .bind("user-1")
    .bind("local")
    .bind("files/file-uploading/content.pdf")
    .bind("content.pdf")
    .bind(100_i64)
    .bind(100_i64)
    .execute(&pool)
    .await
    .unwrap();

    let invalid = sqlx::query(
        r#"
        INSERT INTO files (
            file_id, owner_user_id, project_id, storage_id, object_key,
            original_filename, mime_type, size_bytes, status, metadata_json,
            created_at, updated_at
        ) VALUES (?, ?, NULL, ?, ?, ?, NULL, NULL, 'ready', '{}', ?, ?)
        "#,
    )
    .bind("file-invalid")
    .bind("user-1")
    .bind("local")
    .bind("files/file-invalid/content.pdf")
    .bind("content.pdf")
    .bind(100_i64)
    .bind(100_i64)
    .execute(&pool)
    .await;

    assert!(invalid.is_err());
}

#[tokio::test]
async fn migration_owns_subagent_session_schema() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();

    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();

    let columns = sqlx::query("PRAGMA table_info(sessions)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<std::collections::BTreeSet<_>>();
    for column in [
        "latest_turn_seq",
        "parent_session_id",
        "parent_turn_id",
        "tree_root_session_id",
        "agent_path",
    ] {
        assert!(columns.contains(column));
    }

    let objects = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE name IN ('sessions_tree_agent_path_unique', 'sessions_cleanup_after_delete') ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        objects,
        vec![
            "sessions_cleanup_after_delete".to_string(),
            "sessions_tree_agent_path_unique".to_string(),
        ]
    );
}
