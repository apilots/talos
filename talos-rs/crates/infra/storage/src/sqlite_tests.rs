use super::*;

#[tokio::test]
async fn test_sqlite_storage() {
    let storage = SqliteStorage::new(":memory:").await.unwrap();

    // Test table exists
    let exists = storage.table_exists("sessions").await.unwrap();
    assert!(exists);
    let user_tokens_exists = storage.table_exists("user_tokens").await.unwrap();
    assert!(user_tokens_exists);

    // Test bindings insert and query
    let row = Row::new(
        vec![
            "agent_id".to_string(),
            "match_type".to_string(),
            "priority".to_string(),
            "created_at".to_string(),
        ],
        vec![
            Value::String("main".to_string()),
            Value::String("channel".to_string()),
            Value::Number(10.into()),
            Value::Number((1710460800000i64).into()),
        ],
    );

    let _id = storage.insert("bindings", &row).await.unwrap();

    // Test query without params first
    let rows = storage.query("SELECT * FROM bindings", &[]).await.unwrap();
    assert_eq!(rows.len(), 1);
    // Value is stored as JSON string, so it includes quotes
    assert!(rows[0].get_string("agent_id").unwrap().contains("main"));
}

#[tokio::test]
async fn test_init_schema_adds_title_column_to_legacy_sessions_table() {
    let storage = SqliteStorage::new(":memory:").await.unwrap();

    storage
        .execute("DROP TABLE IF EXISTS messages", &[])
        .await
        .unwrap();
    storage
        .execute("DROP TABLE IF EXISTS sessions", &[])
        .await
        .unwrap();
    storage
        .execute(
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
                    context_json TEXT,
                    model TEXT,
                    message_count INTEGER NOT NULL DEFAULT 0,
                    usage_json TEXT,
                    conf_json TEXT
                )
                "#,
            &[],
        )
        .await
        .unwrap();

    storage.init_schema().await.unwrap();

    let session_columns = storage
        .query("PRAGMA table_info(sessions)", &[])
        .await
        .unwrap();
    let has_title = session_columns
        .iter()
        .any(|row| row.get_string("name").as_deref() == Some("title"));
    assert!(has_title, "sessions table should contain title column");
}
