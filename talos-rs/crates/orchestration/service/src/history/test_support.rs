//! Count real SQLite reads without changing the production storage API.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_storage::{Row, SqliteStorage, Storage, StorageTransaction};

pub(super) struct CountingStorage {
    pub(super) inner: SqliteStorage,
    reads: AtomicUsize,
    notice_reads: AtomicUsize,
    event_rows: AtomicUsize,
    terminal_read_pause: std::sync::Mutex<Option<std::sync::Arc<TerminalReadPause>>>,
}

impl CountingStorage {
    pub(super) fn new(inner: SqliteStorage) -> Self {
        Self {
            inner,
            reads: AtomicUsize::new(0),
            notice_reads: AtomicUsize::new(0),
            event_rows: AtomicUsize::new(0),
            terminal_read_pause: Default::default(),
        }
    }

    pub(super) fn pause_terminal_reads(&self, count: usize) -> std::sync::Arc<TerminalReadPause> {
        let pause = std::sync::Arc::new(TerminalReadPause {
            remaining: AtomicUsize::new(count),
            reached: Default::default(),
            proceed: Default::default(),
        });
        *self.terminal_read_pause.lock().unwrap() = Some(pause.clone());
        pause
    }

    pub(super) fn take_reads(&self) -> usize {
        self.reads.swap(0, Ordering::Relaxed)
    }

    pub(super) fn take_notice_reads(&self) -> usize {
        self.notice_reads.swap(0, Ordering::Relaxed)
    }

    pub(super) fn take_event_rows(&self) -> usize {
        self.event_rows.swap(0, Ordering::Relaxed)
    }
}

pub(super) struct TerminalReadPause {
    remaining: AtomicUsize,
    pub(super) reached: tokio::sync::Notify,
    pub(super) proceed: tokio::sync::Notify,
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
        if sql.contains("notice_position") {
            self.notice_reads.fetch_add(1, Ordering::Relaxed);
        }
        let rows = self.inner.query(sql, params).await?;
        // Pause after terminal selection releases its connection, allowing a
        // real reset transaction to commit before the subsequent event scan.
        if sql.contains("'turnCompleted', 'turnFailed'") {
            let pause = self.terminal_read_pause.lock().unwrap().clone();
            if let Some(pause) = pause {
                if pause
                    .remaining
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                        remaining.checked_sub(1)
                    })
                    .is_ok()
                {
                    pause.reached.notify_one();
                    pause.proceed.notified().await;
                }
            }
        }
        self.event_rows.fetch_add(
            rows.iter()
                .filter(|row| row.get_string("payload_json").is_some())
                .count(),
            Ordering::Relaxed,
        );
        Ok(rows)
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
