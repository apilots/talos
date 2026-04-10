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

#[derive(Clone, Default)]
pub struct MemoryFlushManager {
    tasks: Arc<Mutex<HashMap<String, PendingFlushTask>>>,
    pending_count: Arc<AtomicUsize>,
}

impl MemoryFlushManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn mark_task_completed(pending_count: &AtomicUsize) {
        pending_count.fetch_sub(1, Ordering::SeqCst);
    }

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

    pub async fn drain(&self, timeout: Duration) -> TalosResult<()> {
        let deadline = Instant::now() + timeout;

        loop {
            let pending = {
                let mut tasks = self.tasks.lock().await;
                if tasks.is_empty() {
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
            }
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending_count.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryFlushManager;
    use std::sync::Arc;
    use std::time::Duration;
    use talos_error::TalosError;
    use tokio::sync::{oneshot, Barrier};

    #[tokio::test]
    async fn test_spawn_flush_dedupes_same_session() {
        let manager = MemoryFlushManager::new();
        let (tx, rx) = oneshot::channel::<()>();

        let accepted = manager
            .spawn_flush("session-1", async move {
                let _ = rx.await;
                Ok(())
            })
            .await;
        assert!(accepted);

        let duplicate = manager.spawn_flush("session-1", async { Ok(()) }).await;
        assert!(!duplicate);

        assert_eq!(manager.pending_count(), 1);
        tx.send(()).expect("receiver should still be waiting");
        manager
            .drain(Duration::from_secs(1))
            .await
            .expect("drain should finish");
        assert_eq!(manager.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_drain_waits_for_pending_flushes() {
        let manager = MemoryFlushManager::new();
        manager
            .spawn_flush("session-1", async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(())
            })
            .await;

        manager
            .drain(Duration::from_secs(1))
            .await
            .expect("drain should wait for flush completion");
        assert_eq!(manager.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_drain_times_out_for_stuck_flush() {
        let manager = MemoryFlushManager::new();
        let (tx, rx) = oneshot::channel::<()>();
        manager
            .spawn_flush("session-timeout", async move {
                let _ = rx.await;
                Ok(())
            })
            .await;

        let err = manager
            .drain(Duration::from_millis(50))
            .await
            .expect_err("drain should time out");
        assert!(matches!(err, TalosError::InternalError(_)));
        tx.send(()).expect("receiver should still be waiting");
    }

    #[tokio::test]
    async fn test_flush_failure_is_observed_and_cleared() {
        let manager = MemoryFlushManager::new();
        manager
            .spawn_flush("session-error", async {
                Err(TalosError::ToolError("flush failed".to_string()))
            })
            .await;

        manager
            .drain(Duration::from_secs(1))
            .await
            .expect("failed flush should still join cleanly");
        assert_eq!(manager.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_spawn_flush_dedupes_concurrent_same_session() {
        let manager = Arc::new(MemoryFlushManager::new());
        let barrier = Arc::new(Barrier::new(3));

        let manager_a = manager.clone();
        let barrier_a = barrier.clone();
        let task_a = tokio::spawn(async move {
            barrier_a.wait().await;
            manager_a
                .spawn_flush("session-concurrent", async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok(())
                })
                .await
        });

        let manager_b = manager.clone();
        let barrier_b = barrier.clone();
        let task_b = tokio::spawn(async move {
            barrier_b.wait().await;
            manager_b
                .spawn_flush("session-concurrent", async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok(())
                })
                .await
        });

        barrier.wait().await;
        let accepted_a = task_a.await.expect("first task should join");
        let accepted_b = task_b.await.expect("second task should join");

        assert_ne!(accepted_a, accepted_b);
        assert_eq!(manager.pending_count(), 1);
        manager
            .drain(Duration::from_secs(1))
            .await
            .expect("drain should finish");
        assert_eq!(manager.pending_count(), 0);
    }
}
