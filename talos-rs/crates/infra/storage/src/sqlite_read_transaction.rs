//! Deferred WAL snapshots use SQLx's rollback ownership and never take the writer gate.
use super::{Row, SqliteStorage, StorageTransaction};
use async_trait::async_trait;
use sqlx::{Sqlite, Transaction};
use talos_error::{ErrorCode, TalosError, TalosResult};

#[async_trait]
impl StorageTransaction for Transaction<'static, Sqlite> {
    async fn query(&mut self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param);
        }
        let rows = query
            .fetch_all(&mut **self)
            .await
            .map_err(|error| crate::database_error("query the database read snapshot", error))?;
        Ok(SqliteStorage::rows(rows))
    }

    async fn execute(&mut self, _sql: &str, _params: &[&str]) -> TalosResult<u64> {
        Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Writes are not allowed in a database read snapshot.",
        ))
    }

    async fn commit(self: Box<Self>) -> TalosResult<()> {
        (*self)
            .commit()
            .await
            .map_err(|error| crate::database_error("finish the database read snapshot", error))
    }

    async fn rollback(self: Box<Self>) -> TalosResult<()> {
        (*self)
            .rollback()
            .await
            .map_err(|error| crate::database_error("roll back the database read snapshot", error))
    }
}
