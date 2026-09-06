use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::sync::{oneshot, Notify};
use tokio::task::AbortHandle;

use crate::{
    Cancellation, CancellationMode, RunControlScope, RunTermination, Settlement, SettlementKind,
};

const MAX_RUN_TASKS: usize = 256;
const MAX_CLEANUP_FAILURES: usize = 32;

/// Typed task cleanup failure retained by the owning Run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupFailure {
    /// The task owner explicitly reported that cleanup could not be confirmed.
    OwnerUnconfirmed,
    /// The settlement deadline elapsed before the task completed.
    DeadlineExceeded,
    /// The task was force-aborted after cooperative cancellation elapsed.
    ForceAborted,
    /// The task ended without returning a completion value.
    TaskDropped,
}

/// Owner-confirmed cleanup outcome for one task.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CleanupResult {
    /// The task stopped and released its Run-scoped resources.
    Confirmed,
    /// The task could not confirm complete cleanup.
    Unconfirmed(CleanupFailure),
}

/// Aggregated cleanup result for all tasks owned by a Run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupReport {
    /// Every registered task confirmed cleanup.
    Confirmed,
    /// At least one task did not confirm cleanup.
    Unconfirmed {
        /// Exact number of tasks whose cleanup was not confirmed.
        count: usize,
        /// Bounded task diagnostics.
        failures: Vec<CleanupFailure>,
    },
}

/// Failure to register a new Run task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    /// The Run stopped accepting new tasks.
    Closed,
    /// The Run reached its bounded task capacity.
    CapacityExceeded,
}

/// Failure to receive the output of a registered Run task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTaskError {
    /// The task stopped without returning its business output.
    Stopped,
}

/// Business output and cleanup confirmation returned by one Run task.
#[derive(Debug)]
pub enum RunTaskCompletion<T> {
    /// The task released its Run-scoped resources.
    Confirmed(T),
    /// The task owner could not confirm release of its Run-scoped resources.
    CleanupUnconfirmed(T),
}

/// Awaitable business output for one task owned by `RunTaskGroup`.
pub struct RunTask<T> {
    output: oneshot::Receiver<T>,
}

impl<T> std::fmt::Debug for RunTask<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RunTask")
    }
}

impl<T> Future for RunTask<T> {
    type Output = Result<T, RunTaskError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.output)
            .poll(context)
            .map(|result| result.map_err(|_| RunTaskError::Stopped))
    }
}

struct TaskEntry {
    abort_handle: AbortHandle,
    started_at: Instant,
}

struct RunTaskGroupState {
    accepting: bool,
    tasks: HashMap<u64, TaskEntry>,
    failure_count: usize,
    failures: Vec<CleanupFailure>,
}

struct RunTaskGroupInner {
    scope: RunControlScope,
    cancellation: Cancellation,
    next_id: AtomicU64,
    state: Mutex<RunTaskGroupState>,
    changed: Notify,
}

/// Restricted capability for registering tasks owned by one Run.
///
/// Runtime and Kernel may register tasks. Only `ActiveRunControl` can settle
/// the group, request cancellation, or force-abort unfinished tasks.
#[derive(Clone)]
pub struct RunTaskGroup {
    inner: Arc<RunTaskGroupInner>,
}

impl std::fmt::Debug for RunTaskGroup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RunTaskGroup")
    }
}

impl RunTaskGroup {
    /// Return the cooperative cancellation signal shared by this task group.
    pub fn cancellation(&self) -> Cancellation {
        self.inner.cancellation.clone()
    }

    /// Register one Run-owned task before its factory creates resources.
    pub fn spawn<T, F, Fut>(&self, factory: F) -> Result<RunTask<T>, SpawnError>
    where
        T: Send + 'static,
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: Future<Output = RunTaskCompletion<T>> + Send + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let mut state = self.inner.state.lock().unwrap();
        if !state.accepting {
            return Err(SpawnError::Closed);
        }
        if state.tasks.len() >= MAX_RUN_TASKS {
            return Err(SpawnError::CapacityExceeded);
        }

        let cancellation = self.inner.cancellation.clone();
        let group = self.clone();
        let (output_tx, output_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut guard = TaskGuard::new(group, id);
            let (output, cleanup) = match factory(cancellation).await {
                RunTaskCompletion::Confirmed(output) => (output, CleanupResult::Confirmed),
                RunTaskCompletion::CleanupUnconfirmed(output) => (
                    output,
                    CleanupResult::Unconfirmed(CleanupFailure::OwnerUnconfirmed),
                ),
            };
            guard.complete(cleanup);
            let _ = output_tx.send(output);
        });
        state.tasks.insert(
            id,
            TaskEntry {
                abort_handle: task.abort_handle(),
                started_at: Instant::now(),
            },
        );
        tracing::info!(
            scope_kind = self.inner.scope.kind(),
            session_id = self.inner.scope.session_id(),
            turn_id = self.inner.scope.turn_id(),
            operation_id = self.inner.scope.operation_id(),
            task_id = id,
            active_tasks = state.tasks.len(),
            "run_task_registered"
        );
        drop(task);
        Ok(RunTask { output: output_rx })
    }

    pub(crate) fn new(scope: RunControlScope, cancellation: Cancellation) -> Self {
        Self {
            inner: Arc::new(RunTaskGroupInner {
                scope,
                cancellation,
                next_id: AtomicU64::new(1),
                state: Mutex::new(RunTaskGroupState {
                    accepting: true,
                    tasks: HashMap::new(),
                    failure_count: 0,
                    failures: Vec::new(),
                }),
                changed: Notify::new(),
            }),
        }
    }

    pub(crate) async fn settle(&self, termination: RunTermination) -> Settlement {
        let (timeout, cancellation_requested) = match termination {
            RunTermination::Completed { timeout } => (timeout, false),
            RunTermination::Cancelled { policy } => (policy.grace, true),
        };
        {
            let mut state = self.inner.state.lock().unwrap();
            state.accepting = false;
        }
        if cancellation_requested {
            tracing::info!(
                scope_kind = self.inner.scope.kind(),
                session_id = self.inner.scope.session_id(),
                turn_id = self.inner.scope.turn_id(),
                operation_id = self.inner.scope.operation_id(),
                active_tasks = self.active_count(),
                grace_ms = timeout.as_millis(),
                "run_cancellation_requested"
            );
            self.inner.cancellation.request();
        }

        tracing::info!(
            scope_kind = self.inner.scope.kind(),
            session_id = self.inner.scope.session_id(),
            turn_id = self.inner.scope.turn_id(),
            operation_id = self.inner.scope.operation_id(),
            termination = ?termination,
            active_tasks = self.active_count(),
            "run_settlement_started"
        );

        if let Some(cleanup) = self.wait_for_tasks(timeout).await {
            let kind = match termination {
                RunTermination::Completed { .. } => SettlementKind::Completed,
                RunTermination::Cancelled { .. } => {
                    SettlementKind::Cancelled(CancellationMode::Cooperative)
                }
            };
            return Settlement { kind, cleanup };
        }

        tracing::warn!(
            scope_kind = self.inner.scope.kind(),
            session_id = self.inner.scope.session_id(),
            turn_id = self.inner.scope.turn_id(),
            operation_id = self.inner.scope.operation_id(),
            termination = ?termination,
            active_tasks = self.active_count(),
            elapsed_ms = timeout.as_millis(),
            "run_force_abort_started"
        );
        let reason = match termination {
            RunTermination::Completed { .. } => CleanupFailure::DeadlineExceeded,
            RunTermination::Cancelled { .. } => CleanupFailure::ForceAborted,
        };
        let cleanup = self.abort_unfinished(reason);
        let kind = match termination {
            RunTermination::Completed { .. } => SettlementKind::Completed,
            RunTermination::Cancelled { .. } => SettlementKind::Cancelled(CancellationMode::Forced),
        };
        Settlement { kind, cleanup }
    }

    fn active_count(&self) -> usize {
        self.inner.state.lock().unwrap().tasks.len()
    }

    async fn wait_for_tasks(&self, timeout: Duration) -> Option<CleanupReport> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.inner.changed.notified();
            if self.active_count() == 0 {
                return Some(self.report());
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return None;
            }
        }
    }

    fn report(&self) -> CleanupReport {
        cleanup_report(&self.inner.state.lock().unwrap())
    }

    fn finish(&self, id: u64, result: CleanupResult) {
        let mut state = self.inner.state.lock().unwrap();
        let Some(entry) = state.tasks.remove(&id) else {
            return;
        };
        let cleanup_outcome = match &result {
            CleanupResult::Confirmed => CleanupOutcome::Confirmed,
            CleanupResult::Unconfirmed(_) => CleanupOutcome::Unconfirmed,
        };
        if let CleanupResult::Unconfirmed(failure) = result {
            push_failure(&mut state, failure);
        }
        tracing::info!(
            scope_kind = self.inner.scope.kind(),
            session_id = self.inner.scope.session_id(),
            turn_id = self.inner.scope.turn_id(),
            operation_id = self.inner.scope.operation_id(),
            task_id = id,
            cleanup_outcome = ?cleanup_outcome,
            duration_ms = entry.started_at.elapsed().as_millis(),
            active_tasks = state.tasks.len(),
            "run_task_cleanup_completed"
        );
        drop(state);
        self.inner.changed.notify_waiters();
    }

    fn abort_unfinished(&self, failure: CleanupFailure) -> CleanupReport {
        let mut state = self.inner.state.lock().unwrap();
        let unfinished = state.tasks.drain().collect::<Vec<_>>();
        for (id, entry) in unfinished {
            entry.abort_handle.abort();
            tracing::warn!(
                scope_kind = self.inner.scope.kind(),
                session_id = self.inner.scope.session_id(),
                turn_id = self.inner.scope.turn_id(),
                operation_id = self.inner.scope.operation_id(),
                task_id = id,
                cleanup_outcome = ?CleanupOutcome::Unconfirmed,
                duration_ms = entry.started_at.elapsed().as_millis(),
                active_tasks = state.tasks.len(),
                "run_task_cleanup_completed"
            );
            push_failure(&mut state, failure.clone());
        }
        let report = cleanup_report(&state);
        drop(state);
        self.inner.changed.notify_waiters();
        report
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CleanupOutcome {
    Confirmed,
    Unconfirmed,
}

fn cleanup_report(state: &RunTaskGroupState) -> CleanupReport {
    if state.failure_count == 0 {
        CleanupReport::Confirmed
    } else {
        CleanupReport::Unconfirmed {
            count: state.failure_count,
            failures: state.failures.clone(),
        }
    }
}

fn push_failure(state: &mut RunTaskGroupState, failure: CleanupFailure) {
    state.failure_count += 1;
    if state.failures.len() < MAX_CLEANUP_FAILURES {
        state.failures.push(failure);
    }
}

struct TaskGuard {
    group: RunTaskGroup,
    id: u64,
    completed: bool,
}

impl TaskGuard {
    fn new(group: RunTaskGroup, id: u64) -> Self {
        Self {
            group,
            id,
            completed: false,
        }
    }

    fn complete(&mut self, result: CleanupResult) {
        self.completed = true;
        self.group.finish(self.id, result);
    }
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.group.finish(
                self.id,
                CleanupResult::Unconfirmed(CleanupFailure::TaskDropped),
            );
        }
    }
}
