//! Talos Storage Layer
//!
//! Provides database abstraction for SQLite and PostgreSQL.
//!
//! # Example
//!
//! ```rust,no_run
//! use talos_storage::{Storage, SqliteStorage};
//!
//! #[tokio::main]
//! async fn main() {
//!     let storage = SqliteStorage::new("data.db").await.unwrap();
//!     let rows = storage.query("SELECT * FROM sessions", &[]).await.unwrap();
//! }
//! ```

mod error;
mod sqlite;
pub use error::database_error;

pub use sqlite::{SqliteStorage, SqliteWriteTransaction};

use async_trait::async_trait;
use serde_json::Value;
use talos_config::{DatabaseType, TalosPaths};
use talos_error::{TalosError, TalosResult};

/// Row represents a database row with column names and values
#[derive(Debug, Clone)]
pub struct Row {
    pub columns: Vec<String>,
    pub values: Vec<Value>,
}

impl Row {
    /// Create a new Row
    pub fn new(columns: Vec<String>, values: Vec<Value>) -> Self {
        Self { columns, values }
    }

    /// Get a value by column name
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.columns
            .iter()
            .position(|c| c == column)
            .and_then(|i| self.values.get(i))
    }

    /// Get a string value by column name
    pub fn get_string(&self, column: &str) -> Option<String> {
        self.get(column)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    /// Get an integer value by column name
    pub fn get_i64(&self, column: &str) -> Option<i64> {
        self.get(column).and_then(|v| {
            v.as_i64().or_else(|| {
                v.as_str()
                    .and_then(|s| s.trim_matches('"').parse::<i64>().ok())
            })
        })
    }
}

/// Storage trait - Database abstraction layer
#[async_trait]
pub trait Storage: Send + Sync {
    /// Begin one database transaction pinned to a single connection.
    async fn begin_transaction(&self) -> TalosResult<Box<dyn StorageTransaction>>;

    /// Execute a query and return rows
    async fn query(&self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>>;

    /// Execute a query and return the number of affected rows
    async fn execute(&self, sql: &str, params: &[&str]) -> TalosResult<u64>;

    /// Insert a row and return the last insert id
    async fn insert(&self, table: &str, data: &Row) -> TalosResult<i64>;

    /// Update rows and return the number of affected rows
    async fn update(
        &self,
        table: &str,
        data: &Row,
        where_clause: &str,
        params: &[&str],
    ) -> TalosResult<u64>;

    /// Delete rows and return the number of affected rows
    async fn delete(&self, table: &str, where_clause: &str, params: &[&str]) -> TalosResult<u64>;

    /// Check if a table exists
    async fn table_exists(&self, table: &str) -> TalosResult<bool>;
}

/// Minimal transactional database boundary used by Store-owned atomic writes.
///
/// Business Store modules own SQL and transaction ordering. This interface
/// only guarantees that all calls use one transaction and that completion is
/// explicit; it must not acquire domain-specific helpers.
#[async_trait]
pub trait StorageTransaction: Send {
    /// Execute a query inside this transaction and return its rows.
    async fn query(&mut self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>>;

    /// Execute a statement inside this transaction.
    async fn execute(&mut self, sql: &str, params: &[&str]) -> TalosResult<u64>;

    /// Commit all writes atomically.
    async fn commit(self: Box<Self>) -> TalosResult<()>;

    /// Roll back all writes explicitly.
    async fn rollback(self: Box<Self>) -> TalosResult<()>;
}

/// Storage configuration
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub database_type: DatabaseType,
    pub connection_string: String,
    pub max_connections: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            database_type: DatabaseType::Sqlite,
            connection_string: TalosPaths::resolve()
                .sqlite_db
                .to_string_lossy()
                .to_string(),
            max_connections: 5,
        }
    }
}

/// Create a storage instance from config
pub async fn create_storage(config: &StorageConfig) -> TalosResult<SqliteStorage> {
    match config.database_type {
        DatabaseType::Sqlite => {
            SqliteStorage::new_with_max_connections(
                &config.connection_string,
                config.max_connections,
            )
            .await
        }
        DatabaseType::Postgres => Err(TalosError::internal(
            "Unsupported database type; technical details were withheld.",
        )
        .context(format!(
            "Unsupported database type: {}",
            config.database_type
        ))),
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
