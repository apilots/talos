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
