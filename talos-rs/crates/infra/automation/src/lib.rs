use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StateMutex};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use talos_error::{ApiError, ErrorCode, TalosError, TalosResult};
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type JobFuture = Pin<Box<dyn Future<Output = TalosResult<()>> + Send>>;
type JobTask = Arc<dyn Fn() -> JobFuture + Send + Sync>;

/// Application-loaded durable control and explicitly advertised native capability.
#[derive(Debug, Clone, Copy)]
pub struct AutomationJobControl {
    pub paused: bool,
    pub run_once: bool,
}

/// Observed process-local facts. No finished time means the outcome is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationJobInfo {
    pub name: String,
    pub schedule: String,
    pub timezone: Tz,
    pub paused: bool,
    pub running: bool,
    pub next_due_at: Option<DateTime<Utc>>,
    pub last_finished_at: Option<DateTime<Utc>>,
    pub last_error: Option<ApiError>,
    pub run_once: bool,
}

pub struct AutomationPauseUpdate<'a> {
    pub name: &'a str,
    pub paused: bool,
}

/// A scheduled operation owned by application composition.
pub struct AutomationJob {
    name: String,
    expression: String,
    timezone: Tz,
    run_on_start: bool,
    control: Option<AutomationJobControl>,
    task: JobTask,
}

impl AutomationJob {
    pub fn cron<F, Fut>(
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
            run_on_start: false,
            control: None,
            task: Arc::new(move || Box::pin(task())),
        }
    }

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
        let mut job = Self::cron(name, expression, timezone, task);
        job.run_on_start = true;
        job
    }

    /// Marks an existing native callback manageable; apply stored pause before start.
    /// Internal runtime pumps must remain unmarked and cannot be queried or controlled.
    pub fn with_control(mut self, control: AutomationJobControl) -> Self {
        self.control = Some(control);
        self
    }
}

#[derive(Default)]
struct JobState {
    paused: bool,
    running: bool,
    stopped: bool,
    next_due_at: Option<DateTime<Utc>>,
    last_finished_at: Option<DateTime<Utc>>,
    last_error: Option<ApiError>,
}

struct RegisteredJob {
    job: AutomationJob,
    schedule: Cron,
    state: StateMutex<JobState>,
    changed: watch::Sender<()>,
}

#[derive(Clone, Copy)]
enum Trigger {
    Scheduled,
    Manual,
}

impl RegisteredJob {
    fn snapshot(&self) -> AutomationJobInfo {
        let state = self.state.lock().expect("automation job state");
        AutomationJobInfo {
            name: self.job.name.clone(),
            schedule: self.job.expression.clone(),
            timezone: self.job.timezone,
            paused: state.paused,
            running: state.running,
            next_due_at: state.next_due_at,
            last_finished_at: state.last_finished_at,
            last_error: state.last_error.clone(),
            run_once: self.job.control.is_some_and(|control| control.run_once),
        }
    }

    fn claim(self: &Arc<Self>, trigger: Trigger) -> TalosResult<Option<RunningJob>> {
        let mut state = self.state.lock().expect("automation job state");
        if state.stopped {
            return match trigger {
                Trigger::Scheduled => Ok(None),
                Trigger::Manual => Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Native job execution is shutting down.",
                )),
            };
        }
        if state.paused && matches!(trigger, Trigger::Scheduled) {
            return Ok(None);
        }
        if state.running {
            return match trigger {
                Trigger::Scheduled => Ok(None),
                Trigger::Manual => Err(TalosError::new(
                    ErrorCode::Conflict,
                    "The native job is already running.",
                )),
            };
        }
        state.running = true;
        Ok(Some(RunningJob(self.clone())))
    }
}

struct RunningJob(Arc<RegisteredJob>);
impl RunningJob {
    async fn run(self) {
        let task = self.0.job.task.clone();
        let result = tokio::spawn(async move { task().await })
            .await
            .map_err(task_error)
            .and_then(|result| result);
        let error = result.as_ref().err().map(TalosError::to_api_error);
        {
            let mut state = self.0.state.lock().expect("automation job state");
            state.last_finished_at = Some(Utc::now());
            state.last_error = error;
        }
        if let Err(error) = result {
            talos_logging::report_error(
                &error.context(format!("run automation job {}", self.0.job.name)),
            );
        }
    }
}
impl Drop for RunningJob {
    fn drop(&mut self) {
        self.0.state.lock().expect("automation job state").running = false;
    }
}

/// One application-owned scheduler; all triggers share the same native callback claim.
#[derive(Clone)]
pub struct Automation {
    shutdown: CancellationToken,
    tasks: Arc<Mutex<Option<Vec<JoinHandle<()>>>>>,
    jobs: Arc<Vec<Arc<RegisteredJob>>>,
}

impl Automation {
    /// Validate all jobs before spawning; paused controls suppress startup and cron.
    pub async fn start(jobs: Vec<AutomationJob>) -> TalosResult<Self> {
        let mut registered: Vec<Arc<RegisteredJob>> = Vec::with_capacity(jobs.len());
        for job in jobs {
            if registered
                .iter()
                .any(|existing| existing.job.name == job.name)
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Native job identifiers must be unique.",
                ));
            }
            let schedule = parse_schedule(&job.name, &job.expression)?;
            let paused = job.control.is_some_and(|control| control.paused);
            let next_due_at = if paused {
                None
            } else {
                Some(next_occurrence(&schedule, job.timezone, Utc::now())?)
            };
            registered.push(Arc::new(RegisteredJob {
                job,
                schedule,
                state: StateMutex::new(JobState {
                    paused,
                    next_due_at,
                    ..JobState::default()
                }),
                changed: watch::channel(()).0,
            }));
        }
        let shutdown = CancellationToken::new();
        let tasks = registered
            .iter()
            .map(|job| tokio::spawn(run_scheduled_job(job.clone(), shutdown.child_token())))
            .collect();
        let automation = Self {
            shutdown,
            tasks: Arc::new(Mutex::new(Some(tasks))),
            jobs: Arc::new(registered),
        };
        for job in automation.jobs.iter().filter(|job| job.job.run_on_start) {
            if let Some(running) = job.claim(Trigger::Scheduled)? {
                running.run().await;
            }
        }
        tracing::info!(job_count = automation.jobs.len(), "automation started");
        Ok(automation)
    }

    fn managed_job(&self, name: &str) -> TalosResult<&Arc<RegisteredJob>> {
        self.jobs
            .iter()
            .find(|job| job.job.name == name && job.job.control.is_some())
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ResourceNotFound,
                    "The native job is not available for management.",
                )
            })
    }

    fn ensure_running(&self) -> TalosResult<()> {
        if self.shutdown.is_cancelled() {
            Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Native job execution is shutting down.",
            ))
        } else {
            Ok(())
        }
    }

    /// Returns only explicitly manageable callbacks, never internal runtime pumps.
    pub async fn jobs(&self) -> TalosResult<Vec<AutomationJobInfo>> {
        self.ensure_running()?;
        Ok(self
            .jobs
            .iter()
            .filter(|job| job.job.control.is_some())
            .map(|job| job.snapshot())
            .collect())
    }

    /// The application must durably save control before applying it here.
    /// Pausing affects future admissions; it never interrupts an already running callback.
    pub async fn set_paused(&self, update: AutomationPauseUpdate<'_>) -> TalosResult<()> {
        let _tasks = self.tasks.lock().await;
        self.ensure_running()?;
        let job = self.managed_job(update.name)?;
        let next = if update.paused {
            None
        } else {
            Some(next_occurrence(
                &job.schedule,
                job.job.timezone,
                Utc::now(),
            )?)
        };
        {
            let mut state = job.state.lock().expect("automation job state");
            state.paused = update.paused;
            state.next_due_at = next;
        }
        job.changed.send_replace(());
        Ok(())
    }

    /// Admits once and returns immediately. Manual execution does not resume cron.
    /// Its task belongs to this scheduler's original shutdown/drain lifecycle.
    pub async fn run_once(&self, name: &str) -> TalosResult<()> {
        let mut tasks = self.tasks.lock().await;
        self.ensure_running()?;
        let job = self.managed_job(name)?;
        if !job.job.control.is_some_and(|control| control.run_once) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "This native job does not support manual execution.",
            ));
        }
        reap_finished(tasks.as_mut().expect("running scheduler task collection")).await;
        let running = job
            .claim(Trigger::Manual)?
            .expect("manual claim returns a task or error");
        tasks
            .as_mut()
            .expect("running scheduler task collection")
            .push(tokio::spawn(running.run()));
        Ok(())
    }

    /// Stops future admissions and drains every scheduled/manual callback once.
    pub async fn shutdown(&self) -> TalosResult<()> {
        let mut owned = self.tasks.lock().await;
        let Some(tasks) = owned.take() else {
            return Ok(());
        };
        for job in self.jobs.iter() {
            job.state.lock().expect("automation job state").stopped = true;
        }
        self.shutdown.cancel();
        drop(owned);
        let mut failure = None;
        for task in tasks {
            if let Err(error) = task.await {
                failure.get_or_insert_with(|| task_error(error));
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

async fn reap_finished(tasks: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < tasks.len() {
        if tasks[index].is_finished() {
            if let Err(error) = tasks.swap_remove(index).await {
                talos_logging::report_error(&task_error(error));
            }
        } else {
            index += 1;
        }
    }
}

async fn run_scheduled_job(job: Arc<RegisteredJob>, shutdown: CancellationToken) {
    let mut changed = job.changed.subscribe();
    loop {
        if shutdown.is_cancelled() {
            return;
        }
        let next = {
            let mut state = job.state.lock().expect("automation job state");
            if state.paused {
                state.next_due_at = None;
                None
            } else {
                match next_occurrence(&job.schedule, job.job.timezone, Utc::now()) {
                    Ok(next) => {
                        state.next_due_at = Some(next);
                        Some(next)
                    }
                    Err(error) => {
                        state.next_due_at = None;
                        talos_logging::report_error(
                            &error.context(format!("schedule automation job {}", job.job.name)),
                        );
                        return;
                    }
                }
            }
        };
        if let Some(next) = next {
            let delay = next
                .signed_duration_since(Utc::now())
                .to_std()
                .unwrap_or_default();
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = changed.changed() => continue,
                _ = tokio::time::sleep(delay) => {
                    if shutdown.is_cancelled() { return; }
                    job.state.lock().expect("automation job state").next_due_at = None;
                    match job.claim(Trigger::Scheduled) {
                        Ok(Some(running)) => running.run().await,
                        Ok(None) => {},
                        Err(error) => talos_logging::report_error(&error.context("admit scheduled native job")),
                    }
                }
            }
        } else {
            tokio::select! { _ = shutdown.cancelled() => return, _ = changed.changed() => continue }
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
fn invalid_schedule(name: &str, error: impl std::fmt::Display) -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        format!("invalid automation cron for {name}: {error}"),
    )
}
fn schedule_error(error: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::internal_with_source(
        "automation cron error; technical details were withheld.",
        error,
    )
}
fn task_error(error: tokio::task::JoinError) -> TalosError {
    TalosError::internal_with_source("automation task stopped unexpectedly", error)
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod control_tests;
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
