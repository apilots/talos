use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use talos_error::{TalosError, TalosResult};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

struct PendingFlushTask {
    handle: JoinHandle<()>,
}

/// Tracks Harness-owned asynchronous memory flush tasks.
///
/// The manager is an AgentHarness utility for draining background memory writes
/// during session lifecycle operations. It does not own Service persistence
/// records or product timeline state.
#[derive(Clone, Default)]
pub struct MemoryFlushManager {
    tasks: Arc<Mutex<HashMap<String, PendingFlushTask>>>,
    pending_count: Arc<AtomicUsize>,
}

impl MemoryFlushManager {
    /// Create an empty memory flush manager.
    pub fn new() -> Self {
        Self::default()
    }

    fn mark_task_completed(pending_count: &AtomicUsize) {
        pending_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Start a memory flush for one product session if no flush is pending.
    pub async fn spawn_flush<Fut>(&self, session_id: &str, future: Fut) -> bool
    where
        Fut: Future<Output = TalosResult<()>> + Send + 'static,
    {
        let session_id_owned = session_id.to_string();

        let tasks = self.tasks.clone();
        let pending_count = self.pending_count.clone();
        let task_session_id = session_id_owned.clone();
        let task_handle = tokio::spawn(async move {
            let started_at = Instant::now();
            info!(session_id = %task_session_id, "memory flush started");
            let result = future.await;
            match result {
                Ok(()) => {
                    info!(
                        session_id = %task_session_id,
                        elapsed_ms = started_at.elapsed().as_millis() as u64,
                        "memory flush completed"
                    );
                }
                Err(err) => {
                    warn!(
                        session_id = %task_session_id,
                        elapsed_ms = started_at.elapsed().as_millis() as u64,
                        error = %err,
                        "memory flush failed"
                    );
                }
            }

            let mut tasks = tasks.lock().await;
            if tasks.remove(&task_session_id).is_some() {
                Self::mark_task_completed(pending_count.as_ref());
            }
        });

        let mut tasks = self.tasks.lock().await;
        if tasks.contains_key(&session_id_owned) {
            info!(
                session_id = %session_id_owned,
                "skip duplicate memory flush because one is already pending"
            );
            task_handle.abort();
            return false;
        }
        tasks.insert(
            session_id_owned,
            PendingFlushTask {
                handle: task_handle,
            },
        );
        self.pending_count.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// Wait for all tracked flushes to finish or fail with a timeout error.
    pub async fn drain(&self, timeout: Duration) -> TalosResult<()> {
        let deadline = Instant::now() + timeout;
        let started_at = Instant::now();
        info!(
            pending_count = self.pending_count(),
            timeout_ms = timeout.as_millis() as u64,
            "memory flush drain started"
        );

        loop {
            let pending = {
                let mut tasks = self.tasks.lock().await;
                if tasks.is_empty() {
                    info!(
                        elapsed_ms = started_at.elapsed().as_millis() as u64,
                        "memory flush drain completed"
                    );
                    return Ok(());
                }
                tasks.drain().collect::<Vec<_>>()
            };

            for (session_id, task) in pending {
                let remaining =
                    deadline
                        .checked_duration_since(Instant::now())
                        .ok_or_else(|| {
                            TalosError::InternalError(format!(
                                "memory flush drain timed out waiting for session {}",
                                session_id
                            ))
                        })?;

                tokio::time::timeout(remaining, task.handle)
                    .await
                    .map_err(|_| {
                        TalosError::InternalError(format!(
                            "memory flush drain timed out waiting for session {}",
                            session_id
                        ))
                    })?
                    .map_err(|err| {
                        TalosError::InternalError(format!(
                            "memory flush task join error for session {}: {}",
                            session_id, err
                        ))
                    })?;
                // `drain()` removes the task from the tracking map before join,
                // so the task-side cleanup no longer sees the entry and cannot
                // decrement the tracked count on this path.
                Self::mark_task_completed(self.pending_count.as_ref());
                info!(session_id = %session_id, "memory flush drain joined task");
            }
        }
    }

    /// Return the number of tracked flush tasks that have not completed.
    pub fn pending_count(&self) -> usize {
        self.pending_count.load(Ordering::SeqCst)
    }

    /// Check whether a session currently has a tracked pending flush.
    pub async fn contains_session(&self, session_id: &str) -> bool {
        self.tasks.lock().await.contains_key(session_id)
    }
}

#[cfg(test)]
#[path = "flush_manager_tests.rs"]
mod tests;
