use super::*;
use tokio::task::JoinHandle;
use tokio::time::Instant;

pub(super) struct Entry {
    pub stop: CancellationToken,
    pub control: watch::Sender<Control>,
    join: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Entry {
    pub fn new(
        stop: CancellationToken,
        control: watch::Sender<Control>,
        join: JoinHandle<()>,
    ) -> Self {
        Self {
            stop,
            control,
            join: tokio::sync::Mutex::new(Some(join)),
        }
    }

    pub async fn join(&self) -> TalosResult<()> {
        let mut task = self.join.lock().await;
        if let Some(running) = task.as_mut() {
            let result = running.await.map_err(|error| {
                TalosError::internal_with_source(
                    "Guard task supervisor exited unexpectedly.",
                    error,
                )
            });
            *task = None;
            result?;
        }
        Ok(())
    }
}

#[tracing::instrument(name = "guard_task", skip_all, fields(task_name = spec.name))]
pub(super) async fn supervise<T: GuardTask>(
    spec: GuardTaskSpec,
    task: T,
    stop: CancellationToken,
    mut control: watch::Receiver<Control>,
) {
    tracing::info!(target: "talos::guard_task", task_name = spec.name,
        interval_ms = spec.interval.as_millis() as u64, run_on_start = spec.run_on_start,
        "guard task started");
    let task = Arc::new(task);
    let mut reason = spec.run_on_start.then_some(GuardTaskReason::Startup);
    let mut next = Instant::now() + spec.interval;
    let mut backoff = Duration::from_secs(1);
    let mut request = 0;
    loop {
        if reason.is_none() {
            reason = tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                changed = control.changed() => {
                    if changed.is_err() { break; }
                    let updated = control.borrow_and_update().clone();
                    if updated.request == request {
                        next = Instant::now() + updated.interval;
                        None
                    } else {
                        Some(GuardTaskReason::Requested)
                    }
                },
                _ = tokio::time::sleep_until(next) => Some(GuardTaskReason::Periodic),
            };
        }
        if reason.is_none() {
            continue;
        }
        if stop.is_cancelled() {
            break;
        }
        let current = control.borrow_and_update().clone();
        request = current.request;
        let reason_value = reason.take().expect("guard task has a trigger");
        let context = GuardTaskContext {
            stop: stop.clone(),
            reason: reason_value,
        };
        let started = Instant::now();
        tracing::info!(target: "talos::guard_task", task_name = spec.name, reason = ?reason_value,
            "guard task run started");
        let task = task.clone();
        // Do not abort this child on cancellation: the business round observes
        // its context and finishes its current safe filesystem operation.
        let result = tokio::spawn(async move { task.run(context).await }).await;
        let panicked = result.is_err();
        let success = match result {
            Ok(Ok(())) => true,
            Ok(Err(error)) => {
                talos_logging::report_error(&error);
                false
            }
            Err(error) => {
                talos_logging::report_error(&TalosError::internal_with_source(
                    "Guard task run exited unexpectedly.",
                    error,
                ));
                false
            }
        };
        tracing::info!(target: "talos::guard_task", task_name = spec.name, success,
            cancelled = stop.is_cancelled(), elapsed_ms = started.elapsed().as_millis() as u64,
            "guard task run completed");
        if stop.is_cancelled() {
            break;
        }
        if panicked {
            if started.elapsed() >= Duration::from_secs(60) {
                backoff = Duration::from_secs(1);
            }
            tracing::warn!(target: "talos::guard_task", task_name = spec.name,
                retry_after_secs = backoff.as_secs(), "guard task restarting after failure");
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                _ = tokio::time::sleep(backoff) => {},
            }
            backoff = (backoff * 2).min(Duration::from_secs(60));
            reason = Some(GuardTaskReason::Restart);
        } else {
            backoff = Duration::from_secs(1);
            // Skip missed periods rather than queueing catch-up rounds.
            next = started + current.interval;
            if next <= Instant::now() {
                next = Instant::now() + current.interval;
            }
        }
    }
    tracing::info!(target: "talos::guard_task", task_name = spec.name,
        "guard task stopped");
}
