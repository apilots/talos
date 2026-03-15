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

mod sqlite;

pub use sqlite::SqliteStorage;

use async_trait::async_trait;
use serde_json::Value;
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

    /// Create tables if they don't exist
    async fn init_schema(&self) -> TalosResult<()>;
}

/// Storage configuration
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub database_type: String,
    pub connection_string: String,
    pub max_connections: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            database_type: "sqlite".to_string(),
            connection_string: "talos.db".to_string(),
            max_connections: 5,
        }
    }
}

/// Create a storage instance from config
pub async fn create_storage(config: &StorageConfig) -> TalosResult<Box<dyn Storage>> {
    match config.database_type.as_str() {
        "sqlite" => {
            let storage = SqliteStorage::new(&config.connection_string).await?;
            Ok(Box::new(storage))
        }
        _ => Err(TalosError::StorageError(format!(
            "Unsupported database type: {}",
            config.database_type
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_row_get() {
        let row = Row::new(
            vec!["id".to_string(), "name".to_string()],
            vec![Value::Number(1.into()), Value::String("test".to_string())],
        );

        assert_eq!(row.get_i64("id"), Some(1));
        assert_eq!(row.get_string("name"), Some("test".to_string()));
        assert_eq!(row.get("unknown"), None);
    }
}
