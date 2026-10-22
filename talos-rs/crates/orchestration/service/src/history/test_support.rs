//! Count real SQLite reads without changing the production storage API.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_storage::{Row, SqliteStorage, Storage, StorageTransaction};

pub(super) struct CountingStorage {
    pub(super) inner: SqliteStorage,
    reads: AtomicUsize,
}

impl CountingStorage {
    pub(super) fn new(inner: SqliteStorage) -> Self {
        Self {
            inner,
            reads: AtomicUsize::new(0),
        }
    }

    pub(super) fn take_reads(&self) -> usize {
        self.reads.swap(0, Ordering::Relaxed)
    }
}

#[async_trait]
impl Storage for CountingStorage {
    async fn begin_read_transaction(&self) -> TalosResult<Box<dyn StorageTransaction>> {
        self.inner.begin_read_transaction().await
    }

    async fn begin_transaction(&self) -> TalosResult<Box<dyn StorageTransaction>> {
        self.inner.begin_transaction().await
    }

    async fn query(&self, sql: &str, params: &[&str]) -> TalosResult<Vec<Row>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.query(sql, params).await
    }

    async fn execute(&self, sql: &str, params: &[&str]) -> TalosResult<u64> {
        self.inner.execute(sql, params).await
    }

    async fn insert(&self, table: &str, data: &Row) -> TalosResult<i64> {
        self.inner.insert(table, data).await
    }

    async fn update(
        &self,
        table: &str,
        data: &Row,
        clause: &str,
        params: &[&str],
    ) -> TalosResult<u64> {
        self.inner.update(table, data, clause, params).await
    }

    async fn delete(&self, table: &str, clause: &str, params: &[&str]) -> TalosResult<u64> {
        self.inner.delete(table, clause, params).await
    }

    async fn table_exists(&self, table: &str) -> TalosResult<bool> {
        self.inner.table_exists(table).await
    }
}
