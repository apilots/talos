use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

use crate::task_group::{CleanupOutcome, RunTaskGroup};
use crate::{Cancellation, CleanupReport};

#[repr(u8)]
enum RunPhase {
    Running,
    Settling,
    Settled,
}

/// Exact identity of one active Run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunIdentity {
    /// Product Session that owns the Run.
    pub session_id: String,
    /// Product Turn that identifies the Run.
    pub turn_id: String,
}

impl RunIdentity {
    /// Create an exact Run identity.
    pub fn new(session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
        }
    }
}

/// Time available for cooperative cancellation before force-aborting tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancellationPolicy {
    /// Time allowed for registered tasks to confirm cleanup.
    pub grace: Duration,
}

/// Strong-typed intent for the single Run settlement transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTermination {
    /// Settle after normal execution without broadcasting cancellation.
    Completed {
        /// Time allowed for registered tasks to finish.
        timeout: Duration,
    },
    /// Broadcast cancellation and force-abort tasks after the grace period.
    Cancelled {
        /// Cooperative cancellation policy.
        policy: CancellationPolicy,
    },
}

/// Cancellation path used to settle a Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationMode {
    /// Every task settled before force cancellation was required.
    Cooperative,
    /// The grace deadline elapsed and remaining tasks were force-aborted.
    Forced,
}

/// Reason the lifecycle owner settled the Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementKind {
    /// Runtime execution ended without a cancellation request.
    Completed,
    /// Explicit cancellation ended cooperatively or required force.
    Cancelled(CancellationMode),
}

/// Final immutable lifecycle result returned to every settlement caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    /// Path that produced the final result.
    pub kind: SettlementKind,
    /// Aggregated task cleanup result.
    pub cleanup: CleanupReport,
}

/// Owns the single settlement transition for one active Run.
pub struct ActiveRunControl {
    identity: RunIdentity,
    phase: AtomicU8,
    tasks: RunTaskGroup,
    settlement: OnceLock<Settlement>,
    settled: Notify,
    started_at: Instant,
}

impl ActiveRunControl {
    /// Create a running control that accepts task registration.
    pub fn new(identity: RunIdentity) -> Self {
        let cancellation = Cancellation::new();
        Self {
            tasks: RunTaskGroup::new(identity.clone(), cancellation),
            identity,
            phase: AtomicU8::new(RunPhase::Running as u8),
            settlement: OnceLock::new(),
            settled: Notify::new(),
            started_at: Instant::now(),
        }
    }

    /// Return the restricted task registration capability for this Run.
    pub fn tasks(&self) -> RunTaskGroup {
        self.tasks.clone()
    }

    /// Execute or await the one immutable settlement transition for this Run.
    pub async fn settle(self: &std::sync::Arc<Self>, termination: RunTermination) -> Settlement {
        if self
            .phase
            .compare_exchange(
                RunPhase::Running as u8,
                RunPhase::Settling as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            let control = std::sync::Arc::clone(self);
            let task = tokio::spawn(async move {
                let settlement = control.tasks.settle(termination).await;
                control.finish(settlement);
            });
            drop(task);
        }
        self.wait_for_settlement().await
    }

    fn finish(&self, settlement: Settlement) {
        let _ = self.settlement.set(settlement.clone());
        self.phase.store(RunPhase::Settled as u8, Ordering::SeqCst);
        let (cleanup_outcome, unconfirmed_tasks) = cleanup_observation(&settlement.cleanup);
        tracing::info!(
            session_id = self.identity.session_id,
            turn_id = self.identity.turn_id,
            settlement_kind = ?settlement.kind,
            cleanup_outcome = ?cleanup_outcome,
            unconfirmed_tasks,
            duration_ms = self.started_at.elapsed().as_millis(),
            "run_settled"
        );
        self.settled.notify_waiters();
    }

    async fn wait_for_settlement(&self) -> Settlement {
        loop {
            let notified = self.settled.notified();
            if let Some(settlement) = self.settlement.get() {
                return settlement.clone();
            }
            notified.await;
        }
    }
}

fn cleanup_observation(report: &CleanupReport) -> (CleanupOutcome, usize) {
    match report {
        CleanupReport::Confirmed => (CleanupOutcome::Confirmed, 0),
        CleanupReport::Unconfirmed { count, .. } => (CleanupOutcome::Unconfirmed, *count),
    }
}
