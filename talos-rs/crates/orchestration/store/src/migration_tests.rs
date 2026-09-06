use sqlx::Row;
use std::error::Error;
use std::sync::Arc;
use talos_storage::SqliteStorage;

use super::{run_migrations, test_migrations_dir};

#[tokio::test]
async fn migration_normalizes_session_owned_facts_without_context_or_model_copies() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    let migration_dir = test_migrations_dir();
    let all_migrations = sqlx::migrate::Migrator::new(migration_dir.clone())
        .await
        .unwrap();
    let legacy_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < 202608300003)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrations.run(&pool).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO sessions (
            session_key, user_id, channel, scope, peer_id, agent_id,
            workspace_id, created_at, last_activity, context_json, model,
            conf_json, usage_json
        ) VALUES (
            'session-1', 'user-1', 'legacy-channel', 'api', 'user-1', 'agent-1',
            'workspace-1', 1, 2,
            '{"source_type":"desktop","provider":"sdk","user_id":"user-1","message_id":"request-message","trace_id":"request-trace","channel_id":"channel-1","thread_id":"thread-1","project_root":"/derived","environment":{"directory":"/workspace","connection_mode":"embedded"}}',
            'stale-model',
            '{"model_selection":{"model_ref":{"provider":"openai","id":"conf-model"},"reasoning_effort":null},"user_permission_mode":null}',
            '{"tokens":{"input_tokens":3,"output_tokens":2,"total_tokens":5},"context":null}'
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &migration_dir).await.unwrap();

    let columns = sqlx::query("PRAGMA table_info(sessions)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!columns.contains("context_json"));
    assert!(!columns.contains("model"));
    assert!(!columns.contains("scope"));
    assert!(!columns.contains("peer_id"));
    assert!(!columns.contains("account_id"));
    assert!(columns.contains("source_type"));
    assert!(columns.contains("source_provider"));
    assert!(columns.contains("environment_json"));

    let normalized = sqlx::query_as::<_, (String, String, String, String, String)>(
        r#"
        SELECT source_type, source_provider, channel_id, thread_id, conf_json
        FROM sessions WHERE session_key = 'session-1'
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        normalized,
        (
            "desktop".to_string(),
            "sdk".to_string(),
            "channel-1".to_string(),
            "thread-1".to_string(),
            r#"{"model_selection":{"model_ref":{"provider":"openai","id":"conf-model"},"reasoning_effort":null},"user_permission_mode":null}"#.to_string(),
        )
    );
}

#[tokio::test]
async fn migration_normalizes_turn_idempotency_without_response_blobs() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    let migration_dir = test_migrations_dir();
    let all_migrations = sqlx::migrate::Migrator::new(migration_dir.clone())
        .await
        .unwrap();
    let legacy_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < 202608300002)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrations.run(&pool).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO sessions (
            session_key, user_id, channel, scope, peer_id, agent_id,
            workspace_id, created_at, last_activity, context_json, conf_json, usage_json
        ) VALUES ('session-1', 'user-1', 'http', 'api', 'user-1', 'agent-1',
                  'default', 1, 1,
                  '{"source_type":"http","provider":"http","user_id":"user-1","message_id":"message-1","trace_id":"trace-1"}',
                  '{"model_selection":{"model_ref":{"provider":"anthropic","id":"default"},"reasoning_effort":null},"user_permission_mode":null}',
                  '{"tokens":{"input_tokens":0,"output_tokens":0,"total_tokens":0},"context":null}')
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO agent_events (
            session_id, turn_id, sequence, occurred_at, persisted_at, payload_json
        ) VALUES ('session-1', 'turn-1', 1, 1, 1, '{}')
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO message_idempotency_keys (
            session_id, idempotency_key, request_hash, turn_id, message_id,
            status, result_json, created_at, updated_at, expires_at
        ) VALUES (
            'session-1', 'key-1', 'hash-1', 'turn-1', 'message-1',
            'accepted', '{"request_id":"request-1","message_id":"message-1"}',
            10, 11, 100
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &migration_dir).await.unwrap();

    let record = sqlx::query_as::<_, (String, String, String, String, String, String, i64)>(
        r#"
        SELECT command, request_hash, request_id, turn_id, message_id, outcome,
               committed_event_seq
        FROM agent_command_idempotency
        WHERE session_id = 'session-1' AND idempotency_key = 'key-1'
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        record,
        (
            "turn_submission".to_string(),
            "hash-1".to_string(),
            "request-1".to_string(),
            "turn-1".to_string(),
            "message-1".to_string(),
            "accepted".to_string(),
            1,
        )
    );
    assert!(!sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'message_idempotency_keys')",
    )
    .fetch_one(&pool)
    .await
    .unwrap());
}

#[tokio::test]
async fn migration_backfills_and_requires_session_workspace_binding() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    let migration_dir = test_migrations_dir();
    let all_migrations = sqlx::migrate::Migrator::new(migration_dir.clone())
        .await
        .unwrap();
    let legacy_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < 202608290001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrations.run(&pool).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO sessions (
            session_key, user_id, channel, scope, peer_id, agent_id,
            created_at, last_activity, workspace_id, context_json, conf_json, usage_json
        ) VALUES
            ('session-unbound', 'user-1', 'http', 'api', 'user-1', 'agent-a', 1, 1, NULL,
             '{"source_type":"http","provider":"http","user_id":"user-1","message_id":"message-1","trace_id":"trace-1"}',
             '{"model_selection":{"model_ref":{"provider":"anthropic","id":"default"},"reasoning_effort":null},"user_permission_mode":null}',
             '{"tokens":{"input_tokens":0,"output_tokens":0,"total_tokens":0},"context":null}'),
            ('session-bound', 'user-1', 'http', 'api', 'user-1', 'agent-b', 1, 1, 'workspace-alpha',
             '{"source_type":"http","provider":"http","user_id":"user-1","message_id":"message-2","trace_id":"trace-2"}',
             '{"model_selection":{"model_ref":{"provider":"anthropic","id":"default"},"reasoning_effort":null},"user_permission_mode":null}',
             '{"tokens":{"input_tokens":0,"output_tokens":0,"total_tokens":0},"context":null}')
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &migration_dir).await.unwrap();

    let bindings = sqlx::query_as::<_, (String, String, String)>(
        "SELECT session_key, agent_id, workspace_id FROM sessions ORDER BY session_key",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        bindings,
        vec![
            (
                "session-bound".to_string(),
                "agent-b".to_string(),
                "workspace-alpha".to_string(),
            ),
            (
                "session-unbound".to_string(),
                "agent-a".to_string(),
                "default".to_string(),
            ),
        ]
    );

    let workspace_column = sqlx::query("PRAGMA table_info(sessions)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.get::<String, _>("name") == "workspace_id")
        .unwrap();
    assert_eq!(workspace_column.get::<i64, _>("notnull"), 1);
    assert_eq!(
        workspace_column.get::<Option<String>, _>("dflt_value"),
        None
    );
    let table_sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'sessions'",
    )
    .fetch_one(&pool)
    .await
    .expect("read rebuilt sessions schema");
    assert!(table_sql.contains("CHECK (length(trim(workspace_id)) > 0)"));
}

#[tokio::test]
async fn migration_wraps_legacy_session_token_usage_without_losing_totals() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    let migration_dir = test_migrations_dir();
    let all_migrations = sqlx::migrate::Migrator::new(migration_dir.clone())
        .await
        .unwrap();
    let legacy_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < 202608190001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrations.run(&pool).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO sessions (
            session_key, user_id, channel, scope, peer_id, agent_id,
            created_at, last_activity, usage_json, context_json, conf_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("session-legacy-usage")
    .bind("user-1")
    .bind("http")
    .bind("api")
    .bind("user-1")
    .bind("talos")
    .bind(1_i64)
    .bind(1_i64)
    .bind(r#"{"input_tokens":21,"output_tokens":8,"total_tokens":29}"#)
    .bind(r#"{"source_type":"http","provider":"http","user_id":"user-1","message_id":"message-1","trace_id":"trace-1"}"#)
    .bind(r#"{"model_selection":{"model_ref":{"provider":"anthropic","id":"default"},"reasoning_effort":null},"user_permission_mode":null}"#)
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &migration_dir).await.unwrap();

    let usage_json =
        sqlx::query_scalar::<_, String>("SELECT usage_json FROM sessions WHERE session_key = ?")
            .bind("session-legacy-usage")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&usage_json).unwrap(),
        serde_json::json!({
            "tokens": {
                "input_tokens": 21,
                "output_tokens": 8,
                "total_tokens": 29
            },
            "context": null
        })
    );
}

#[tokio::test]
async fn migration_rejects_unsettled_legacy_active_turns() {
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

    let error = run_migrations(&pool, &test_migrations_dir())
        .await
        .expect_err("unsettled legacy Turn must block migration");

    let migration_error = error
        .source()
        .and_then(|source| source.downcast_ref::<sqlx::migrate::MigrateError>())
        .expect("migration failure must retain the typed SQLx source");
    let sqlx::migrate::MigrateError::Execute(sqlx::Error::Database(database_error)) =
        migration_error
    else {
        panic!("migration must fail with a database constraint");
    };
    assert_eq!(
        database_error.kind(),
        sqlx::error::ErrorKind::CheckViolation
    );
}

#[tokio::test]
async fn migration_archives_legacy_history_without_blocking_upgrade() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    let pool = storage.pool().clone();
    let migration_dir = test_migrations_dir();
    let all_migrations = sqlx::migrate::Migrator::new(migration_dir.clone())
        .await
        .unwrap();
    let legacy_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < 202608300003)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrations.run(&pool).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO active_turns (
            session_id, turn_id, message_id, status, last_event_seq,
            current_action_id, updated_at
        ) VALUES ('settled-session', 'turn-1', 'message-1', 'completed', 1, NULL, 10)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO timeline_events (
            session_id, turn_id, message_id, sequence, event_type,
            payload_json, created_at_ms
        ) VALUES (
            'settled-session', 'turn-1', 'message-1', 1, 'turn_started',
            '{}', 10
        )
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &migration_dir).await.unwrap();

    let retained_event: (i64, String, String, String, i64, String, String, i64) = sqlx::query_as(
        r#"
            SELECT event_seq, session_id, turn_id, message_id, sequence,
                   event_type, payload_json, created_at_ms
            FROM legacy_timeline_events
            "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        retained_event,
        (
            1,
            "settled-session".to_string(),
            "turn-1".to_string(),
            "message-1".to_string(),
            1,
            "turn_started".to_string(),
            "{}".to_string(),
            10,
        )
    );
}

#[tokio::test]
async fn migration_archives_empty_timeline_and_drops_settled_execution_state() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    let pool = storage.pool().clone();
    let migration_dir = test_migrations_dir();
    let all_migrations = sqlx::migrate::Migrator::new(migration_dir.clone())
        .await
        .unwrap();
    let legacy_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < 202608300003)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrations.run(&pool).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO active_turns (
            session_id, turn_id, message_id, status, last_event_seq,
            current_action_id, updated_at
        ) VALUES ('settled-session', 'turn-1', 'message-1', 'completed', 1, NULL, 10)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &migration_dir).await.unwrap();

    let removed_tables = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('timeline_events', 'active_turns', 'user_actions') ORDER BY name",
    )
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert!(removed_tables.is_empty());
    let archived_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM legacy_timeline_events")
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(archived_events, 0);
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
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('sessions', 'bindings', 'projects', 'users', 'user_tokens', 'timeline_events', 'active_turns', 'agent_command_idempotency', 'user_actions') ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        vec![
            "agent_command_idempotency".to_string(),
            "bindings".to_string(),
            "projects".to_string(),
            "sessions".to_string(),
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
        "INSERT INTO sessions (session_key, user_id, channel, scope, peer_id, agent_id, created_at, last_activity, message_count, memory_processed_until_seq, context_json, conf_json, usage_json) VALUES ('session-1', 'user-1', 'http', 'api', 'user-1', 'talos', 1, 1, 5, 4, '{\"source_type\":\"http\",\"provider\":\"http\",\"user_id\":\"user-1\",\"message_id\":\"message-1\",\"trace_id\":\"trace-1\"}', '{\"model_selection\":{\"model_ref\":{\"provider\":\"anthropic\",\"id\":\"default\"},\"reasoning_effort\":null},\"user_permission_mode\":null}', '{\"tokens\":{\"input_tokens\":0,\"output_tokens\":0,\"total_tokens\":0},\"context\":null}')",
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

    let migration_dir = test_migrations_dir();
    let all_migrations = sqlx::migrate::Migrator::new(migration_dir.clone())
        .await
        .unwrap();
    let legacy_migrations = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all_migrations
                .iter()
                .filter(|migration| migration.version < 202608230001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrations.run(&pool).await.unwrap();
    sqlx::query(
        r#"
        INSERT INTO profile_turn_usage (
            user_id, session_id, turn_id, last_request_id, model_provider, model_id,
            input_tokens, output_tokens, total_tokens, usage_available, finalized,
            usage_date, timezone, completed_at_ms
        ) VALUES ('user-1', 'session-1', 'turn-1', 'request-1', 'deepseek',
                  'deepseek-v4-flash', 11, 3, 14, 1, 1, '2026-08-23', 'UTC', 1000)
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    run_migrations(&pool, &migration_dir).await.unwrap();
    run_migrations(&pool, &migration_dir).await.unwrap();

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
    let columns = sqlx::query("PRAGMA table_info(profile_turn_usage)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<std::collections::BTreeSet<_>>();
    let model_call_index_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_profile_model_call_usage_request'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let legacy_row = sqlx::query(
        "SELECT usage_kind, session_id, turn_id, total_tokens FROM profile_turn_usage WHERE last_request_id = 'request-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (
            index_count,
            model_call_index_count,
            columns.contains("usage_kind"),
            legacy_row.get::<String, _>("usage_kind"),
            legacy_row.get::<String, _>("session_id"),
            legacy_row.get::<String, _>("turn_id"),
            legacy_row.get::<i64, _>("total_tokens"),
        ),
        (
            1,
            1,
            true,
            "turn".to_string(),
            "session-1".to_string(),
            "turn-1".to_string(),
            14,
        )
    );
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
