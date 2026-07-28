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

        Ok(Self { pool })
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
}

#[cfg(test)]
#[path = "sqlite_tests.rs"]
mod tests;
