//! SQLite Storage Implementation

use super::{Row, Storage, StorageTransaction};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::pool::PoolConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Column, Pool, Row as SqlxRow, Sqlite, SqliteConnection, ValueRef};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tracing::{debug, info};

#[path = "sqlite_read_transaction.rs"]
mod read_transaction;

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// SQLite Storage implementation
pub struct SqliteStorage {
    pool: Pool<Sqlite>,
    write_gate: std::sync::Arc<Mutex<()>>,
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
                    let diagnostic = format!(
                        "Failed to create database directory {}: {}",
                        parent.display(),
                        err
                    );
                    TalosError::internal_with_source(
                        "Failed to create database directory; technical details were withheld.",
                        err,
                    )
                    .context(diagnostic)
                })?;
            }
        }

        let connection_options = SqliteConnectOptions::from_str(&connection_string)
            .map_err(|error| {
                crate::database_error("parse the SQLite connection configuration", error)
            })?
            .busy_timeout(SQLITE_BUSY_TIMEOUT)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(connection_options)
            .await
            .map_err(|e| crate::database_error("connect to the database", e))?;

        info!("SQLite connection established");

        Ok(Self {
            pool,
            write_gate: std::sync::Arc::new(Mutex::new(())),
        })
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

    fn rows(rows: Vec<sqlx::sqlite::SqliteRow>) -> Vec<Row> {
        rows.into_iter().map(sqlite_row).collect()
    }
}

/// SQLite write transaction that acquires the database writer before its first read.
///
/// Store implementations use this boundary for read-modify-write operations so
/// a deferred transaction cannot fail while upgrading a stale read snapshot.
pub struct SqliteWriteTransaction {
    connection: Option<PoolConnection<Sqlite>>,
}

impl SqliteWriteTransaction {
    /// Acquire one pooled connection and begin an immediate SQLite transaction.
    pub async fn begin(pool: &Pool<Sqlite>) -> TalosResult<Self> {
        let connection = pool
            .acquire()
            .await
            .map_err(|error| crate::database_error("acquire a database connection", error))?;
        // SQLx's SQLite worker can finish BEGIN after this future is cancelled.
        // Own cleanup before the await so an unconfirmed transaction is closed,
        // never returned to the pool with its writer lock still held.
        let mut transaction = Self {
            connection: Some(connection),
        };
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *transaction)
            .await
            .map_err(|error| crate::database_error("begin the database transaction", error))?;
        Ok(transaction)
    }

    /// Commit the transaction and return its clean connection to the pool.
    pub async fn commit(mut self) -> TalosResult<()> {
        sqlx::query("COMMIT")
            .execute(&mut **self.connection_mut()?)
            .await
            .map_err(|error| crate::database_error("commit the database transaction", error))?;
        self.connection.take();
        Ok(())
    }

    /// Roll back the transaction and return its clean connection to the pool.
    pub async fn rollback(mut self) -> TalosResult<()> {
        sqlx::query("ROLLBACK")
            .execute(&mut **self.connection_mut()?)
            .await
            .map_err(|error| crate::database_error("roll back the database transaction", error))?;
        self.connection.take();
        Ok(())
    }

    fn connection_mut(&mut self) -> TalosResult<&mut PoolConnection<Sqlite>> {
        self.connection
            .as_mut()
            .ok_or_else(|| TalosError::internal("transaction already finished"))
    }
}

impl Deref for SqliteWriteTransaction {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        self.connection
            .as_deref()
            .expect("active SQLite transaction connection")
    }
}

impl DerefMut for SqliteWriteTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection
            .as_deref_mut()
            .expect("active SQLite transaction connection")
    }
}

impl Drop for SqliteWriteTransaction {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            drop(connection.detach());
        }
    }
}

struct SqliteStorageTransaction {
    transaction: Option<SqliteWriteTransaction>,
    _write_guard: OwnedMutexGuard<()>,
}

#[async_trait]
impl StorageTransaction for SqliteStorageTransaction {
    async fn query(&mut self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }
        let rows = query
            .fetch_all(&mut **self.transaction_mut()?)
            .await
            .map_err(|error| {
                crate::database_error("query within the database transaction", error)
            })?;
        Ok(SqliteStorage::rows(rows))
    }

    async fn execute(&mut self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }
        query
            .execute(&mut **self.transaction_mut()?)
            .await
            .map(|result| result.rows_affected())
            .map_err(|error| crate::database_error("execute the database transaction", error))
    }

    async fn commit(mut self: Box<Self>) -> TalosResult<()> {
        self.transaction
            .take()
            .expect("transaction can finish only once")
            .commit()
            .await
    }

    async fn rollback(mut self: Box<Self>) -> TalosResult<()> {
        self.transaction
            .take()
            .expect("transaction can finish only once")
            .rollback()
            .await
    }
}

impl SqliteStorageTransaction {
    fn transaction_mut(&mut self) -> TalosResult<&mut SqliteWriteTransaction> {
        self.transaction
            .as_mut()
            .ok_or_else(|| TalosError::internal("transaction already finished"))
    }
}

fn sqlite_row(row: sqlx::sqlite::SqliteRow) -> Row {
    let columns = row
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect::<Vec<_>>();
    let values = (0..columns.len())
        .map(|index| {
            if row
                .try_get_raw(index)
                .expect("column indices originate from the same row")
                .is_null()
            {
                Value::Null
            } else if let Ok(value) = row.try_get::<String, _>(index) {
                Value::String(value)
            } else if let Ok(value) = row.try_get::<i64, _>(index) {
                Value::Number(value.into())
            } else if let Ok(value) = row.try_get::<f64, _>(index) {
                serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
            } else if let Ok(value) = row.try_get::<bool, _>(index) {
                Value::Bool(value)
            } else {
                Value::Null
            }
        })
        .collect();
    Row::new(columns, values)
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn begin_read_transaction(&self) -> TalosResult<Box<dyn StorageTransaction>> {
        let transaction =
            self.pool.begin().await.map_err(|error| {
                crate::database_error("begin the database read snapshot", error)
            })?;
        Ok(Box::new(transaction))
    }

    async fn begin_transaction(&self) -> TalosResult<Box<dyn StorageTransaction>> {
        let write_guard = self.write_gate.clone().lock_owned().await;
        let transaction = SqliteWriteTransaction::begin(&self.pool).await?;
        Ok(Box::new(SqliteStorageTransaction {
            transaction: Some(transaction),
            _write_guard: write_guard,
        }))
    }

    async fn query(&self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        debug!("Executing query: {} with params: {:?}", sql, params);

        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| crate::database_error("query the database", e))?;

        let result = Self::rows(rows);

        debug!("Query returned {} rows", result.len());
        Ok(result)
    }

    async fn execute(&self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        debug!("Executing: {} with params: {:?}", sql, params);
        let _write_guard = self.write_gate.lock().await;

        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }

        let result = query
            .execute(&self.pool)
            .await
            .map_err(|e| crate::database_error("execute the database operation", e))?;

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

#[async_trait]
impl StorageTransaction for SqliteWriteTransaction {
    async fn query(&mut self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(*param);
        }
        query
            .fetch_all(&mut **self)
            .await
            .map(SqliteStorage::rows)
            .map_err(|error| crate::database_error("query the database transaction", error))
    }
    async fn execute(&mut self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(*param);
        }
        query
            .execute(&mut **self)
            .await
            .map(|result| result.rows_affected())
            .map_err(|error| crate::database_error("execute the database transaction", error))
    }
    async fn commit(self: Box<Self>) -> TalosResult<()> {
        (*self).commit().await
    }
    async fn rollback(self: Box<Self>) -> TalosResult<()> {
        (*self).rollback().await
    }
}
