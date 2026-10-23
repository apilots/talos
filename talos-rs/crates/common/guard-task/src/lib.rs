//! Supervised periodic work owned by a process lifecycle.
//!
//! Register after constructing task dependencies. Keep one shared `GuardTasks`
//! owner and call `shutdown` before releasing resources used by its tasks.
//! Tasks own business batching and must observe cancellation between safe steps;
//! the runner never aborts an in-flight filesystem commit. There is no persistent
//! scheduler, distributed execution, business retry queue or configuration store.
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

mod runner;

/// Static task identity and cadence. Names must be unique within an owner.
pub struct GuardTaskSpec {
    pub name: &'static str,
    pub interval: Duration,
    pub run_on_start: bool,
}

/// One complete business round. Implementations must be safe to run again after
/// an interruption and check the context before starting another batch.
pub trait GuardTask: Send + Sync + 'static {
    fn run(&self, context: GuardTaskContext) -> impl Future<Output = TalosResult<()>> + Send;
}

/// Execution cancellation and trigger, independent of business configuration.
#[derive(Clone)]
pub struct GuardTaskContext {
    stop: CancellationToken,
    reason: GuardTaskReason,
}

impl GuardTaskContext {
    pub fn is_cancelled(&self) -> bool {
        self.stop.is_cancelled()
    }

    pub async fn cancelled(&self) {
        self.stop.cancelled().await;
    }

    pub fn reason(&self) -> GuardTaskReason {
        self.reason
    }
}

#[derive(Clone, Copy, Debug)]
pub enum GuardTaskReason {
    Startup,
    Periodic,
    Requested,
    Restart,
}

#[derive(Clone)]
struct Control {
    interval: Duration,
    request: u64,
}

/// A task control handle. Updates after shutdown never restart the task.
#[derive(Clone)]
pub struct GuardTaskHandle(Arc<runner::Entry>);

impl GuardTaskHandle {
    /// Changes the cadence without requesting an extra business round.
    pub fn set_interval(&self, interval: Duration) -> TalosResult<()> {
        validate_interval(interval)?;
        self.0.control.send_if_modified(|control| {
            if control.interval == interval {
                return false;
            }
            control.interval = interval;
            true
        });
        Ok(())
    }

    /// Coalesces requests during a running round into one subsequent round.
    pub fn wake(&self) {
        self.0.control.send_modify(|control| {
            control.request = control.request.wrapping_add(1);
        });
    }
}

#[derive(Default)]
struct State {
    closed: bool,
    entries: BTreeMap<&'static str, Arc<runner::Entry>>,
}

/// One process-owned group. Dropping cancels; explicit shutdown also joins.
#[derive(Default)]
pub struct GuardTasks {
    state: Mutex<State>,
}

impl GuardTasks {
    /// Starts a task once; registration is forbidden after shutdown begins.
    pub fn register<T: GuardTask>(
        &self,
        spec: GuardTaskSpec,
        task: T,
    ) -> TalosResult<GuardTaskHandle> {
        validate_interval(spec.interval)?;
        let runtime = tokio::runtime::Handle::try_current().map_err(|error| {
            TalosError::internal_with_source(
                "Guard task registration requires an active Tokio runtime.",
                error,
            )
        })?;
        let mut state = self
            .state
            .lock()
            .expect("Guard task registry lock poisoned");
        if state.closed || spec.name.trim().is_empty() || state.entries.contains_key(spec.name) {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Guard task registration requires an open owner and a unique nonempty name.",
            ));
        }
        let (control, receiver) = watch::channel(Control {
            interval: spec.interval,
            request: 0,
        });
        let stop = CancellationToken::new();
        let name = spec.name;
        let running = runtime.spawn(runner::supervise(spec, task, stop.clone(), receiver));
        let entry = Arc::new(runner::Entry::new(stop, control, running));
        state.entries.insert(name, entry.clone());
        Ok(GuardTaskHandle(entry))
    }

    /// Cancels all tasks before joining any. Concurrent/cancelled callers share
    /// the same joins; returning success means every round has actually stopped.
    pub async fn shutdown(&self) -> TalosResult<()> {
        let entries = {
            let mut state = self
                .state
                .lock()
                .expect("Guard task registry lock poisoned");
            state.closed = true;
            state.entries.values().cloned().collect::<Vec<_>>()
        };
        for entry in &entries {
            entry.stop.cancel();
        }
        let mut result = Ok(());
        for entry in entries {
            let stopped = entry.join().await;
            match (result.is_ok(), stopped) {
                (true, Err(error)) => result = Err(error),
                (false, Err(error)) => talos_logging::report_error(&error),
                (_, Ok(())) => {}
            }
        }
        result
    }
}

impl Drop for GuardTasks {
    fn drop(&mut self) {
        for entry in self
            .state
            .get_mut()
            .expect("Guard task registry lock poisoned")
            .entries
            .values()
        {
            entry.stop.cancel();
        }
    }
}

fn validate_interval(interval: Duration) -> TalosResult<()> {
    if interval.is_zero() || tokio::time::Instant::now().checked_add(interval).is_none() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Guard task interval must be positive and fit the runtime timer.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
