use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use talos_types::AgentTurnRef;
use tokio::sync::Notify;

use crate::task_group::{CleanupOutcome, RunTaskGroup};
use crate::{Cancellation, CleanupReport};

#[repr(u8)]
enum RunPhase {
    Running,
    Settling,
    Settled,
}

/// Typed ownership scope for one controlled operation.
///
/// This type distinguishes product Turn execution from Session-scoped
/// maintenance and process-scoped shutdown without inventing fake Turn IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunControlScope {
    /// One exact product Turn.
    Turn(AgentTurnRef),
    /// One transient operation against Provider-owned Session resources.
    SessionOperation {
        /// Product Session that owns the resources.
        session_id: String,
        /// Process-local operation identity allocated by the lifecycle owner.
        operation_id: String,
    },
    /// One transient process-wide operation such as Provider shutdown.
    ProcessOperation {
        /// Process-local operation identity allocated by the lifecycle owner.
        operation_id: String,
    },
}

impl RunControlScope {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Turn(_) => "turn",
            Self::SessionOperation { .. } => "sessionOperation",
            Self::ProcessOperation { .. } => "processOperation",
        }
    }

    pub(crate) fn session_id(&self) -> Option<&str> {
        match self {
            Self::Turn(identity) => Some(&identity.session_id),
            Self::SessionOperation { session_id, .. } => Some(session_id),
            Self::ProcessOperation { .. } => None,
        }
    }

    pub(crate) fn turn_id(&self) -> Option<&str> {
        match self {
            Self::Turn(identity) => Some(&identity.turn_id),
            Self::SessionOperation { .. } | Self::ProcessOperation { .. } => None,
        }
    }

    pub(crate) fn operation_id(&self) -> Option<&str> {
        match self {
            Self::Turn(_) => None,
            Self::SessionOperation { operation_id, .. }
            | Self::ProcessOperation { operation_id } => Some(operation_id),
        }
    }
}

impl From<AgentTurnRef> for RunControlScope {
    fn from(identity: AgentTurnRef) -> Self {
        Self::Turn(identity)
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
    scope: RunControlScope,
    phase: AtomicU8,
    tasks: RunTaskGroup,
    settlement: OnceLock<Settlement>,
    settled: Notify,
    started_at: Instant,
}

impl ActiveRunControl {
    /// Create a running control that accepts task registration.
    pub fn new(scope: impl Into<RunControlScope>) -> Self {
        let scope = scope.into();
        let cancellation = Cancellation::new();
        Self {
            tasks: RunTaskGroup::new(scope.clone(), cancellation.clone()),
            scope,
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

    /// Return the restricted cooperative cancellation signal for this operation.
    pub fn cancellation(&self) -> Cancellation {
        self.tasks.cancellation()
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
            scope_kind = self.scope.kind(),
            session_id = self.scope.session_id(),
            turn_id = self.scope.turn_id(),
            operation_id = self.scope.operation_id(),
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
