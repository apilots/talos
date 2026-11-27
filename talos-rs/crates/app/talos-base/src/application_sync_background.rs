//! Account maintenance is owned by the host, never awaited by snapshot queries.
use super::application_host::ApplicationHost;
use super::application_management_host::RefreshPhase;
use std::collections::HashMap;
use talos_error::{ErrorCode, TalosError};
use talos_service::Actor;
use tokio::task::JoinHandle;

#[derive(Default)]
pub(crate) struct RefreshTasks {
    closed: bool,
    tasks: HashMap<String, RefreshTask>,
    paused: HashMap<String, usize>,
    suspended: HashMap<String, Option<String>>,
}

/// Serial lifecycle mutations keep resume calls from replacing a cancelled task
/// until its future and the mutation have both finished.
pub(crate) struct RefreshPause<'a> {
    host: &'a ApplicationHost,
    account_id: String,
}

impl Drop for RefreshPause<'_> {
    fn drop(&mut self) {
        let mut background = self
            .host
            .background
            .lock()
            .expect("Sync refresh tasks lock");
        if let Some(count) = background.paused.get_mut(&self.account_id) {
            *count -= 1;
            if *count == 0 {
                background.paused.remove(&self.account_id);
            }
        }
    }
}

struct RefreshTask {
    task: JoinHandle<()>,
    actor: tokio::sync::watch::Sender<Actor>,
}

impl Drop for RefreshTasks {
    fn drop(&mut self) {
        for refresh in self.tasks.values() {
            refresh.task.abort();
        }
    }
}

struct RefreshLifetime(String);

#[derive(Debug)]
enum RefreshWake {
    Started,
    AccountOrComputerChanged,
    TemporaryFailureRetry,
    WorkerStateChanged,
}

impl Drop for RefreshLifetime {
    fn drop(&mut self) {
        tracing::info!(account_id = %self.0, "Account host maintenance stopped");
    }
}

impl ApplicationHost {
    /// Startup/login resumes one account supervisor. Later authentication updates
    /// credentials without replacing its Sync handle; queries never call this.
    pub(crate) fn schedule_refresh(&self, actor: &Actor) {
        if actor.auth_disabled || actor.access_token.is_none() {
            return;
        }
        let mut background = self.background.lock().expect("Sync refresh tasks lock");
        if background.closed || background.paused.contains_key(&actor.user_id) {
            return;
        }
        if let Some(token) = background.suspended.get(&actor.user_id) {
            if token.is_none() || token == &actor.access_token {
                return;
            }
            // A new verified credential can restore a Web account after logout.
            background.suspended.remove(&actor.user_id);
        }
        self.start_refresh(actor, &mut background);
    }

    /// Mutation completion can restore maintenance only while the account remains active.
    /// Unlike authentication restoration, it must never clear a suspension for a newer token.
    #[cfg(any(feature = "sync", test))]
    pub(crate) fn schedule_active_refresh(&self, actor: &Actor) {
        let mut background = self.background.lock().expect("Sync refresh tasks lock");
        if background.suspended.contains_key(&actor.user_id) {
            return;
        }
        self.start_refresh(actor, &mut background);
    }

    fn start_refresh(&self, actor: &Actor, background: &mut RefreshTasks) {
        if actor.auth_disabled
            || actor.access_token.is_none()
            || background.closed
            || background.paused.contains_key(&actor.user_id)
        {
            return;
        }
        if let Some(refresh) = background.tasks.get(&actor.user_id) {
            if !refresh.task.is_finished() {
                refresh.actor.send_if_modified(|current| {
                    if current == actor {
                        return false;
                    }
                    *current = actor.clone();
                    true
                });
                return;
            }
        }
        let host = self.self_weak.clone();
        let account_id = actor.user_id.clone();
        let (actor, updates) = tokio::sync::watch::channel(actor.clone());
        let failures = self.worker_failures.subscribe();
        tracing::info!(%account_id, "Account host maintenance started");
        let lifetime = RefreshLifetime(account_id.clone());
        let task = tokio::spawn(maintain_account(host, updates, failures, lifetime));
        background
            .tasks
            .insert(account_id, RefreshTask { task, actor });
    }

    /// Durable Computer imports wake registration reconciliation, never a read API.
    #[cfg(any(feature = "sync", all(test, feature = "server")))]
    pub(crate) fn notify_refresh(&self, account_id: &str) {
        if let Some(refresh) = self
            .background
            .lock()
            .expect("Sync refresh tasks lock")
            .tasks
            .get(account_id)
        {
            refresh.actor.send_modify(|_| {});
        }
    }

    pub(crate) async fn pause_refresh(&self, account_id: &str) -> RefreshPause<'_> {
        let task = {
            let mut background = self.background.lock().expect("Sync refresh tasks lock");
            *background.paused.entry(account_id.into()).or_default() += 1;
            let task = background.tasks.remove(account_id);
            if let Some(task) = &task {
                task.task.abort();
            }
            task
        };
        // Establish the guard before awaiting so request cancellation releases it.
        let pause = RefreshPause {
            host: self,
            account_id: account_id.into(),
        };
        if let Some(task) = task {
            stop(task.task).await;
        }
        pause
    }

    #[cfg(feature = "sync")]
    pub(crate) fn refreshes_closed(&self) -> bool {
        self.background
            .lock()
            .expect("Sync refresh tasks lock")
            .closed
    }

    pub(crate) fn account_connections_allowed(&self, account_id: &str) -> bool {
        let background = self.background.lock().expect("Sync refresh tasks lock");
        !background.closed && !background.suspended.contains_key(account_id)
    }

    pub(crate) fn resume_login(&self, actor: &Actor) {
        self.background
            .lock()
            .expect("Sync refresh tasks lock")
            .suspended
            .remove(&actor.user_id);
        self.schedule_refresh(actor);
    }

    pub(crate) async fn suspend_refresh(&self, actor: &Actor) -> RefreshPause<'_> {
        {
            let mut background = self.background.lock().expect("Sync refresh tasks lock");
            let token = actor.access_token.clone().or_else(|| {
                background
                    .tasks
                    .get(&actor.user_id)
                    .and_then(|refresh| refresh.actor.borrow().access_token.clone())
            });
            background.suspended.insert(actor.user_id.clone(), token);
        }
        self.pause_refresh(&actor.user_id).await
    }

    pub(crate) async fn shutdown_refreshes(&self) {
        let tasks = {
            let mut background = self.background.lock().expect("Sync refresh tasks lock");
            background.closed = true;
            std::mem::take(&mut background.tasks)
        };
        for task in tasks.into_values() {
            stop(task.task).await;
        }
    }
}

async fn maintain_account(
    host: std::sync::Weak<ApplicationHost>,
    mut updates: tokio::sync::watch::Receiver<Actor>,
    mut failures: tokio::sync::watch::Receiver<Option<talos_error::ApiError>>,
    _lifetime: RefreshLifetime,
) {
    let _directory = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
        super::application_computer_directory::maintain(host.clone(), updates.clone()),
    ));
    #[cfg(feature = "sync")]
    let _hub_recovery = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
        super::application_sync_hub_recovery::maintain_hub_identity(host.clone(), updates.clone()),
    ));
    let mut wake_reason = RefreshWake::Started;
    loop {
        let Some(owner) = host.upgrade() else { break };
        let actor = updates.borrow_and_update().clone();
        failures.borrow_and_update();
        let started = std::time::Instant::now();
        let mut phase = RefreshPhase::AwaitLifecycle;
        let result = {
            let _guard = owner.lifecycle.lock().await;
            async {
                owner.refresh_account(&actor, &mut phase).await?;
                phase = RefreshPhase::ObserveWorker;
                Ok::<_, talos_error::TalosError>(())
            }
            .await
        };
        let failed = result.is_err();
        let retryable = result.as_ref().is_err_and(refresh_retryable);
        let retry_after_seconds = retryable.then_some(5_u64);
        match result {
            Err(error) => {
                let span = tracing::warn_span!("account_host_initialization_failed", account_id = %actor.user_id, ?phase, ?wake_reason, elapsed_ms = started.elapsed().as_millis() as u64, retry_after_seconds);
                let _entered = span.enter();
                talos_logging::report_error(&error);
                owner
                    .failures
                    .write()
                    .expect("sync failures lock")
                    .insert(actor.user_id.clone(), error.to_api_error());
            }
            Ok(()) => {
                owner
                    .failures
                    .write()
                    .expect("sync failures lock")
                    .remove(&actor.user_id);
                tracing::info!(account_id = %actor.user_id, ?wake_reason, elapsed_ms = started.elapsed().as_millis() as u64, "Account host initialized; waiting for lifecycle changes");
            }
        };
        drop(owner);
        if failed {
            wake_reason = tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)), if retryable => RefreshWake::TemporaryFailureRetry,
                changed = updates.changed() => {
                    if changed.is_err() { break }
                    RefreshWake::AccountOrComputerChanged
                },
                // Ignore our own failure publication. Only successful explicit
                // Worker recovery clears it and can unblock terminal enrollment.
                recovered = async {
                    loop {
                        failures.changed().await?;
                        if failures.borrow_and_update().is_none() {
                            return Ok::<_, tokio::sync::watch::error::RecvError>(());
                        }
                    }
                } => {
                    if recovered.is_err() { break }
                    RefreshWake::WorkerStateChanged
                },
            };
            continue;
        }
        wake_reason = tokio::select! {
            changed = updates.changed() => {
                if changed.is_err() { break }
                RefreshWake::AccountOrComputerChanged
            },
            // Explicit credential recovery clears the registration error;
            // it wakes reconciliation just like a terminal failure.
            changed = failures.changed() => {
                if changed.is_err() { break }
                RefreshWake::WorkerStateChanged
            },

        };
    }
}

pub(crate) fn refresh_retryable(error: &TalosError) -> bool {
    matches!(
        error.error_code(),
        ErrorCode::ServiceUnavailable | ErrorCode::Timeout | ErrorCode::UsageLimitExceeded
    )
}

async fn stop(task: JoinHandle<()>) {
    task.abort();
    if let Err(error) = task.await {
        if !error.is_cancelled() {
            talos_logging::report_error(&talos_error::TalosError::internal_with_source(
                "The synchronization maintenance task failed.",
                error,
            ));
        }
    }
}

#[cfg(test)]
#[path = "application_sync_background_tests.rs"]
mod tests;
