//! SQLite Storage Implementation

use super::{Row, Storage};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Column, Pool, Row as SqlxRow, Sqlite};
use std::path::Path;
use talos_error::{TalosError, TalosResult};
use tracing::{debug, info};

/// SQLite Storage implementation
pub struct SqliteStorage {
    pool: Pool<Sqlite>,
}

impl SqliteStorage {
    /// Create a new SQLite storage
    pub async fn new(database_path: &str) -> TalosResult<Self> {
        Self::new_with_max_connections(database_path, 5).await
    }

    /// Creates SQLite storage with an explicit connection-pool size.
    pub async fn new_with_max_connections(
        database_path: &str,
        max_connections: u32,
    ) -> TalosResult<Self> {
        info!("Connecting to SQLite database: {}", database_path);

        let connection_string = Self::normalize_connection_string(database_path);

        if let Some(parent) = Self::database_parent_dir(database_path) {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    TalosError::StorageError(format!(
                        "Failed to create database directory {}: {}",
                        parent.display(),
                        err
                    ))
                })?;
            }
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect(&connection_string)
            .await
            .map_err(|e| {
                TalosError::StorageError(format!("Failed to connect to database: {}", e))
            })?;

        info!("SQLite connection established");

        let storage = Self { pool };

        // Initialize schema
        storage.init_schema().await?;

        Ok(storage)
    }

    /// Get the connection pool
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    fn value_to_param(value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => {
                if *b {
                    "1".to_string()
                } else {
                    "0".to_string()
                }
            }
            Value::Null => String::new(),
            _ => value.to_string(),
        }
    }

    fn normalize_connection_string(database_path: &str) -> String {
        if database_path == ":memory:"
            || database_path == "sqlite::memory:"
            || database_path.starts_with("sqlite:")
        {
            return database_path.to_string();
        }

        let path = Path::new(database_path);
        let rendered = path.to_string_lossy();
        format!("sqlite://{rendered}?mode=rwc")
    }

    fn database_parent_dir(database_path: &str) -> Option<&Path> {
        if database_path == ":memory:"
            || database_path == "sqlite::memory:"
            || database_path.starts_with("sqlite:")
        {
            return None;
        }

        Path::new(database_path).parent()
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn query(&self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        debug!("Executing query: {} with params: {:?}", sql, params);

        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| TalosError::StorageError(format!("Query failed: {}", e)))?;

        let mut result = Vec::new();
        for row in rows {
            let columns: Vec<String> = row.columns().iter().map(|c| c.name().to_string()).collect();
            let mut values = Vec::new();

            for i in 0..columns.len() {
                let json_value = if let Ok(value) = row.try_get::<String, _>(i) {
                    Value::String(value)
                } else if let Ok(value) = row.try_get::<i64, _>(i) {
                    Value::Number(value.into())
                } else if let Ok(value) = row.try_get::<f64, _>(i) {
                    match serde_json::Number::from_f64(value) {
                        Some(num) => Value::Number(num),
                        None => Value::Null,
                    }
                } else if let Ok(value) = row.try_get::<bool, _>(i) {
                    Value::Bool(value)
                } else {
                    Value::Null
                };
                values.push(json_value);
            }

            result.push(Row::new(columns, values));
        }

        debug!("Query returned {} rows", result.len());
        Ok(result)
    }

    async fn execute(&self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        debug!("Executing: {} with params: {:?}", sql, params);

        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }

        let result = query
            .execute(&self.pool)
            .await
            .map_err(|e| TalosError::StorageError(format!("Execute failed: {}", e)))?;

        Ok(result.rows_affected())
    }

    async fn insert(&self, table: &str, data: &Row) -> TalosResult<i64> {
        let columns = data.columns.join(", ");
        let placeholders = (0..data.values.len())
            .map(|i| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table, columns, placeholders
        );

        let params: Vec<String> = data.values.iter().map(Self::value_to_param).collect();
        let params_ref: Vec<&str> = params.iter().map(|s| s.as_str()).collect();

        self.execute(&sql, &params_ref).await?;

        // Get last insert id
        let row = self.query("SELECT last_insert_rowid()", &[]).await?;
        let id = row
            .first()
            .and_then(|r| r.get_i64("last_insert_rowid()"))
            .unwrap_or(0);

        Ok(id)
    }

    async fn update(
        &self,
        table: &str,
        data: &Row,
        where_clause: &str,
        params: &[&str],
    ) -> TalosResult<u64> {
        let set_clause = data
            .columns
            .iter()
            .zip(data.values.iter())
            .map(|(col, _)| format!("{} = ?", col))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!("UPDATE {} SET {} WHERE {}", table, set_clause, where_clause);

        // Build params: data values + where params
        let mut all_params: Vec<String> = data.values.iter().map(Self::value_to_param).collect();
        all_params.extend(params.iter().map(|s| s.to_string()));
        let all_params_ref: Vec<&str> = all_params.iter().map(|s| s.as_str()).collect();

        self.execute(&sql, &all_params_ref).await
    }

    async fn delete(&self, table: &str, where_clause: &str, params: &[&str]) -> TalosResult<u64> {
        let sql = format!("DELETE FROM {} WHERE {}", table, where_clause);
        self.execute(&sql, params).await
    }

    async fn table_exists(&self, table: &str) -> TalosResult<bool> {
        let sql = "SELECT name FROM sqlite_master WHERE type='table' AND name=?";
        let rows = self.query(sql, &[table]).await?;
        Ok(!rows.is_empty())
    }

    async fn init_schema(&self) -> TalosResult<()> {
        info!("Initializing database schema");

        if self.table_exists("sessions").await? {
            let session_columns = self.query("PRAGMA table_info(sessions)", &[]).await?;
            let mut existing = std::collections::HashSet::new();
            for row in session_columns {
                if let Some(name) = row.get_string("name") {
                    existing.insert(name);
                }
            }

            let required = [
                "context_json",
                "model",
                "message_count",
                "usage_json",
                "conf_json",
            ];
            let needs_rebuild = required.iter().any(|name| !existing.contains(*name));
            if needs_rebuild {
                info!("Rebuilding legacy sessions schema");
                self.execute("DROP TABLE IF EXISTS messages", &[]).await?;
                self.execute("DROP TABLE IF EXISTS sessions", &[]).await?;
            } else if !existing.contains("title") {
                info!("Adding title column to sessions schema");
                self.execute(
                    "ALTER TABLE sessions ADD COLUMN title TEXT NOT NULL DEFAULT 'Untitled Session'",
                    &[],
                )
                .await?;
            }
            if !existing.contains("project_id") {
                info!("Adding project_id column to sessions schema");
                self.execute("ALTER TABLE sessions ADD COLUMN project_id TEXT", &[])
                    .await?;
            }
            if !existing.contains("current_directory") {
                info!("Adding current_directory column to sessions schema");
                self.execute(
                    "ALTER TABLE sessions ADD COLUMN current_directory TEXT",
                    &[],
                )
                .await?;
            }
        }

        // Sessions table
        let create_sessions = r#"
            CREATE TABLE IF NOT EXISTS sessions (
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
                workspace_id TEXT,
                project_id TEXT,
                current_directory TEXT
            )
        "#;

        self.execute(create_sessions, &[]).await?;
        debug!("Sessions table created/verified");

        if self.table_exists("sessions").await? {
            let session_columns = self.query("PRAGMA table_info(sessions)", &[]).await?;
            let mut existing = std::collections::HashSet::new();
            for row in session_columns {
                if let Some(name) = row.get_string("name") {
                    existing.insert(name);
                }
            }
            if !existing.contains("workspace_id") {
                info!("Adding workspace_id column to sessions schema");
                self.execute("ALTER TABLE sessions ADD COLUMN workspace_id TEXT", &[])
                    .await?;
            }
        }

        // Bindings table
        let create_bindings = r#"
            CREATE TABLE IF NOT EXISTS bindings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id TEXT NOT NULL,
                match_type TEXT NOT NULL,
                match_kind TEXT,
                match_id TEXT,
                channel TEXT,
                priority INTEGER NOT NULL DEFAULT 0,
                tags TEXT,
                created_at INTEGER NOT NULL
            )
        "#;

        self.execute(create_bindings, &[]).await?;
        debug!("Bindings table created/verified");

        if self.table_exists("projects").await? {
            let project_columns = self.query("PRAGMA table_info(projects)", &[]).await?;
            let mut existing = std::collections::HashSet::new();
            for row in project_columns {
                if let Some(name) = row.get_string("name") {
                    existing.insert(name);
                }
            }
            if !existing.contains("user_id") {
                info!("Adding user_id column to projects schema");
                self.execute("ALTER TABLE projects ADD COLUMN user_id TEXT", &[])
                    .await?;
                self.execute(
                    "UPDATE projects SET user_id = COALESCE(NULLIF(owner_user_id, ''), '') WHERE user_id IS NULL OR user_id = ''",
                    &[],
                )
                .await?;
            }
        }

        let create_projects = r#"
            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                user_id TEXT NOT NULL,
                directory TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
        "#;

        self.execute(create_projects, &[]).await?;
        debug!("Projects table created/verified");

        if self.table_exists("users").await? {
            let user_columns = self.query("PRAGMA table_info(users)", &[]).await?;
            let mut existing = std::collections::HashSet::new();
            for row in user_columns {
                if let Some(name) = row.get_string("name") {
                    existing.insert(name);
                }
            }
            if !existing.contains("email") {
                info!("Adding email column to users schema");
                self.execute("ALTER TABLE users ADD COLUMN email TEXT", &[])
                    .await?;
            }
        }

        let create_users = r#"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT UNIQUE NOT NULL,
                username TEXT UNIQUE NOT NULL,
                email TEXT,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL,
                token_version INTEGER NOT NULL,
                disabled INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )
        "#;

        self.execute(create_users, &[]).await?;
        debug!("Users table created/verified");

        let create_user_tokens = r#"
            CREATE TABLE IF NOT EXISTS user_tokens (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_id TEXT UNIQUE NOT NULL,
                user_id TEXT NOT NULL,
                name TEXT NOT NULL,
                token_hash TEXT UNIQUE NOT NULL,
                prefix TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER,
                expires_at INTEGER
            )
        "#;

        self.execute(create_user_tokens, &[]).await?;
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_tokens_user_id ON user_tokens(user_id)",
            &[],
        )
        .await?;
        self.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_user_tokens_token_hash ON user_tokens(token_hash)",
            &[],
        )
        .await?;
        debug!("User tokens table created/verified");

        let create_timeline_events = r#"
            CREATE TABLE IF NOT EXISTS timeline_events (
                event_seq INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            )
        "#;

        self.execute(create_timeline_events, &[]).await?;
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_timeline_events_session_seq ON timeline_events(session_id, event_seq)",
            &[],
        )
        .await?;
        self.execute(
            "CREATE INDEX IF NOT EXISTS idx_timeline_events_turn ON timeline_events(session_id, turn_id)",
            &[],
        )
        .await?;
        debug!("Timeline events table created/verified");

        let create_active_turns = r#"
            CREATE TABLE IF NOT EXISTS active_turns (
                session_id TEXT PRIMARY KEY,
                turn_id TEXT NOT NULL,
                message_id TEXT,
                status TEXT NOT NULL,
                last_event_seq INTEGER,
                pending_user_action_json TEXT,
                updated_at INTEGER NOT NULL
            )
        "#;

        self.execute(create_active_turns, &[]).await?;
        debug!("Active turns table created/verified");

        if self.table_exists("message_idempotency_keys").await? {
            let idempotency_columns = self
                .query("PRAGMA table_info(message_idempotency_keys)", &[])
                .await?;
            let mut existing = std::collections::HashSet::new();
            for row in idempotency_columns {
                if let Some(name) = row.get_string("name") {
                    existing.insert(name);
                }
            }
            if existing.contains("ack_json") && !existing.contains("result_json") {
                info!("Renaming idempotency ack_json column to result_json");
                self.execute(
                    "ALTER TABLE message_idempotency_keys RENAME COLUMN ack_json TO result_json",
                    &[],
                )
                .await?;
            }
        }

        let create_message_idempotency_keys = r#"
            CREATE TABLE IF NOT EXISTS message_idempotency_keys (
                session_id TEXT NOT NULL,
                idempotency_key TEXT NOT NULL,
                request_hash TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                message_id TEXT,
                status TEXT NOT NULL,
                result_json TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                expires_at INTEGER,
                PRIMARY KEY (session_id, idempotency_key)
            )
        "#;

        self.execute(create_message_idempotency_keys, &[]).await?;
        debug!("Message idempotency table created/verified");

        info!("Database schema initialization complete");
        Ok(())
    }
}

#[cfg(test)]
#[path = "sqlite_tests.rs"]
mod tests;
