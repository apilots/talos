use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use talos_error::{TalosError, TalosResult};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type JobFuture = Pin<Box<dyn Future<Output = TalosResult<()>> + Send>>;
type JobTask = Arc<dyn Fn() -> JobFuture + Send + Sync>;

/// A scheduled operation owned by application composition.
pub struct AutomationJob {
    name: String,
    expression: String,
    timezone: Tz,
    run_on_start: bool,
    task: JobTask,
}

impl AutomationJob {
    /// Creates a cron job that also runs once after automation starts.
    pub fn cron_on_start<F, Fut>(
        name: impl Into<String>,
        expression: impl Into<String>,
        timezone: Tz,
        task: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = TalosResult<()>> + Send + 'static,
    {
        Self {
            name: name.into(),
            expression: expression.into(),
            timezone,
            run_on_start: true,
            task: Arc::new(move || Box::pin(task())),
        }
    }
}

/// Owns scheduled job execution and its idempotent shutdown lifecycle.
#[derive(Clone)]
pub struct Automation {
    shutdown: CancellationToken,
    tasks: Arc<Mutex<Option<Vec<JoinHandle<()>>>>>,
}

impl Automation {
    /// Starts supplied jobs and evaluates each cron occurrence in its IANA timezone.
    pub async fn start(jobs: Vec<AutomationJob>) -> TalosResult<Self> {
        let job_names: Vec<String> = jobs.iter().map(|job| job.name.clone()).collect();
        let mut startup_jobs = Vec::new();
        let shutdown = CancellationToken::new();
        let mut tasks = Vec::with_capacity(jobs.len());

        for job in jobs {
            let schedule = parse_schedule(&job.name, &job.expression)?;
            let running = Arc::new(AtomicBool::new(false));
            if job.run_on_start {
                startup_jobs.push((job.name.clone(), job.task.clone(), running.clone()));
            }

            tasks.push(tokio::spawn(run_scheduled_job(
                job.name,
                schedule,
                job.timezone,
                job.task,
                running,
                shutdown.child_token(),
            )));
        }

        tracing::info!(job_count = job_names.len(), jobs = ?job_names, "automation started");

        for (name, task, running) in startup_jobs {
            run_job(name, task, running).await;
        }

        Ok(Self {
            shutdown,
            tasks: Arc::new(Mutex::new(Some(tasks))),
        })
    }

    /// Stops automation once; later calls are successful no-ops.
    pub async fn shutdown(&self) -> TalosResult<()> {
        let Some(tasks) = self.tasks.lock().await.take() else {
            return Ok(());
        };
        self.shutdown.cancel();
        for task in tasks {
            task.await.map_err(task_error)?;
        }
        Ok(())
    }
}

async fn run_scheduled_job(
    name: String,
    schedule: Cron,
    timezone: Tz,
    task: JobTask,
    running: Arc<AtomicBool>,
    shutdown: CancellationToken,
) {
    loop {
        let next = match next_occurrence(&schedule, timezone, Utc::now()) {
            Ok(next) => next,
            Err(error) => {
                tracing::error!(job = %name, %error, "automation job schedule failed");
                return;
            }
        };
        let delay = next
            .signed_duration_since(Utc::now())
            .to_std()
            .unwrap_or_default();

        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(delay) => run_job(name.clone(), task.clone(), running.clone()).await,
        }
    }
}

fn parse_schedule(name: &str, expression: &str) -> TalosResult<Cron> {
    let mut schedule = Cron::new(expression);
    schedule.with_seconds_required().with_dom_and_dow();
    schedule
        .parse()
        .map_err(|error| invalid_schedule(name, error))
}

fn next_occurrence(
    schedule: &Cron,
    timezone: Tz,
    now: DateTime<Utc>,
) -> TalosResult<DateTime<Utc>> {
    schedule
        .find_next_occurrence(&now.with_timezone(&timezone), false)
        .map(|next| next.with_timezone(&Utc))
        .map_err(schedule_error)
}

async fn run_job(name: String, task: JobTask, running: Arc<AtomicBool>) {
    if running.swap(true, Ordering::AcqRel) {
        tracing::warn!(job = %name, "automation job overlap skipped");
        return;
    }
    if let Err(error) = task().await {
        tracing::warn!(job = %name, %error, "automation job failed");
    }
    running.store(false, Ordering::Release);
}

fn invalid_schedule(error_name: &str, error: impl std::fmt::Display) -> TalosError {
    TalosError::InvalidConfiguration(format!("invalid automation cron for {error_name}: {error}"))
}

fn schedule_error(error: impl std::fmt::Display) -> TalosError {
    TalosError::InternalError(format!("automation cron error: {error}"))
}

fn task_error(error: tokio::task::JoinError) -> TalosError {
    TalosError::InternalError(format!("automation task stopped unexpectedly: {error}"))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
