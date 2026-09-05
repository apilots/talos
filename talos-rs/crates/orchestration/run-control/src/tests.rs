use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;

use crate::{
    ActiveRunControl, CancellationMode, CancellationPolicy, CleanupFailure, CleanupReport,
    RunControlScope, RunTaskCompletion, RunTaskError, RunTermination, Settlement, SettlementKind,
};
use talos_types::AgentTurnRef;

#[tokio::test]
async fn completed_settlement_waits_for_registered_task() {
    let control = Arc::new(ActiveRunControl::new(AgentTurnRef::new(
        "session-1",
        "turn-1",
    )));
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let run_task = control
        .tasks()
        .spawn(move |_| async move {
            let _ = started_tx.send(());
            let _ = release_rx.await;
            RunTaskCompletion::Confirmed(42)
        })
        .unwrap();

    let settling_control = control.clone();
    let settlement = tokio::spawn(async move {
        settling_control
            .settle(RunTermination::Completed {
                timeout: Duration::from_secs(1),
            })
            .await
    });
    started_rx.await.unwrap();
    assert!(!settlement.is_finished());
    release_tx.send(()).unwrap();

    assert_eq!(run_task.await, Ok(42));
    assert_eq!(
        settlement.await.unwrap(),
        Settlement {
            kind: SettlementKind::Completed,
            cleanup: CleanupReport::Confirmed,
        }
    );
}

#[tokio::test]
async fn cancelled_settlement_notifies_registered_task() {
    let control = Arc::new(ActiveRunControl::new(AgentTurnRef::new(
        "session-1",
        "turn-1",
    )));
    let run_task = control
        .tasks()
        .spawn(|cancellation| async move {
            cancellation.requested().await;
            RunTaskCompletion::Confirmed("cancelled")
        })
        .unwrap();

    let settlement = control
        .settle(RunTermination::Cancelled {
            policy: CancellationPolicy {
                grace: Duration::from_secs(1),
            },
        })
        .await;

    assert_eq!(run_task.await, Ok("cancelled"));
    assert_eq!(
        settlement,
        Settlement {
            kind: SettlementKind::Cancelled(CancellationMode::Cooperative),
            cleanup: CleanupReport::Confirmed,
        }
    );
}

#[tokio::test]
async fn cancelled_settlement_force_aborts_unfinished_task() {
    let control = Arc::new(ActiveRunControl::new(AgentTurnRef::new(
        "session-1",
        "turn-1",
    )));
    let run_task = control
        .tasks()
        .spawn(|_| async move {
            std::future::pending::<()>().await;
            RunTaskCompletion::Confirmed(())
        })
        .unwrap();

    let settlement = control
        .settle(RunTermination::Cancelled {
            policy: CancellationPolicy {
                grace: Duration::from_millis(10),
            },
        })
        .await;

    assert_eq!(run_task.await, Err(RunTaskError::Stopped));
    assert_eq!(
        settlement,
        Settlement {
            kind: SettlementKind::Cancelled(CancellationMode::Forced),
            cleanup: CleanupReport::Unconfirmed {
                count: 1,
                failures: vec![CleanupFailure::ForceAborted],
            },
        }
    );
}

#[tokio::test]
async fn repeated_settlement_returns_the_first_result() {
    let control = Arc::new(ActiveRunControl::new(AgentTurnRef::new(
        "session-1",
        "turn-1",
    )));

    let expected = control
        .settle(RunTermination::Completed {
            timeout: Duration::from_secs(1),
        })
        .await;
    let repeated = control
        .settle(RunTermination::Cancelled {
            policy: CancellationPolicy {
                grace: Duration::from_secs(1),
            },
        })
        .await;

    assert_eq!(repeated, expected);
}

#[tokio::test]
async fn session_operation_uses_the_same_control_without_a_fake_turn() {
    let control = Arc::new(ActiveRunControl::new(RunControlScope::SessionOperation {
        session_id: "session-1".to_owned(),
        operation_id: "clear-1".to_owned(),
    }));
    let task = control
        .tasks()
        .spawn(|cancellation| async move {
            cancellation.requested().await;
            RunTaskCompletion::Confirmed(())
        })
        .unwrap();

    let settlement = control
        .settle(RunTermination::Cancelled {
            policy: CancellationPolicy {
                grace: Duration::from_secs(1),
            },
        })
        .await;

    assert_eq!(task.await, Ok(()));
    assert_eq!(
        settlement,
        Settlement {
            kind: SettlementKind::Cancelled(CancellationMode::Cooperative),
            cleanup: CleanupReport::Confirmed,
        }
    );
}
