//! Account maintenance is owned by the host, never awaited by snapshot queries.
use super::application_sync::ApplicationSyncHost;
use super::application_sync_management::RefreshPhase;
use std::collections::HashMap;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use tokio::{sync::watch, task::JoinHandle};

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
    host: &'a ApplicationSyncHost,
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
    state: watch::Sender<RefreshState>,
}

struct RefreshState {
    actor: Actor,
    revision: u64,
    computer: Option<LocalComputerDecision>,
}

impl RefreshState {
    fn new(actor: Actor) -> Self {
        Self {
            actor,
            revision: 0,
            computer: None,
        }
    }

    fn request_reconciliation(&mut self) {
        self.revision += 1;
        self.computer = None;
    }
}

#[derive(Clone, Copy)]
pub(crate) enum LocalComputerDecision {
    Owned,
    Denied,
    Failed(ErrorCode),
}

pub(crate) struct LocalComputerReconciliation<'a> {
    state: &'a watch::Sender<RefreshState>,
    revision: u64,
}

impl LocalComputerReconciliation<'_> {
    /// Publish only the current revision's first definitive ownership result.
    pub(crate) fn complete(&self, result: Result<bool, &TalosError>) {
        let decision = match result {
            Ok(true) => LocalComputerDecision::Owned,
            Ok(false) => LocalComputerDecision::Denied,
            Err(error) if retryable(error) => return,
            Err(error)
                if matches!(
                    error.error_code(),
                    ErrorCode::ResourceNotFound | ErrorCode::PermissionDenied
                ) =>
            {
                LocalComputerDecision::Denied
            }
            Err(error) => LocalComputerDecision::Failed(error.error_code()),
        };
        self.state.send_if_modified(|current| {
            if current.revision != self.revision || current.computer.is_some() {
                return false;
            }
            current.computer = Some(decision);
            true
        });
    }
}

fn retryable(error: &TalosError) -> bool {
    matches!(
        error.error_code(),
        ErrorCode::ServiceUnavailable | ErrorCode::Timeout | ErrorCode::UsageLimitExceeded
    )
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
    WorkerConnectionEnded,
}

impl Drop for RefreshLifetime {
    fn drop(&mut self) {
        tracing::info!(account_id = %self.0, "Sync account maintenance stopped");
    }
}

impl ApplicationSyncHost {
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
        if let Some(refresh) = background.tasks.get(&actor.user_id) {
            if !refresh.task.is_finished() {
                refresh.state.send_if_modified(|current| {
                    // Cached and verified Actors can differ only in their display name.
                    if current.actor.access_token == actor.access_token
                        && current.actor.role == actor.role
                    {
                        return false;
                    }
                    current.actor = actor.clone();
                    current.request_reconciliation();
                    true
                });
                return;
            }
        }
        let host = self.self_weak.clone();
        let account_id = actor.user_id.clone();
        let (state, updates) = watch::channel(RefreshState::new(actor.clone()));
        let failures = self.worker_failures.subscribe();
        tracing::info!(%account_id, "Sync account maintenance started");
        let lifetime = RefreshLifetime(account_id.clone());
        let task = tokio::spawn(maintain_account(
            host,
            state.clone(),
            updates,
            failures,
            lifetime,
        ));
        background
            .tasks
            .insert(account_id, RefreshTask { task, state });
    }

    /// Wait for the existing account supervisor's ownership decision.
    /// An absent supervisor leaves routing to the existing authorization path.
    /// A cancelled supervisor denies the waiting request.
    pub(crate) async fn await_local_computer_reconciliation(
        &self,
        actor: &Actor,
    ) -> TalosResult<Option<LocalComputerDecision>> {
        let Some(mut updates) = self
            .background
            .lock()
            .expect("Sync refresh tasks lock")
            .tasks
            .get(&actor.user_id)
            .filter(|refresh| !refresh.task.is_finished())
            .map(|refresh| refresh.state.subscribe())
        else {
            return Ok(None);
        };
        self.verified_actor(actor).await?;
        // This deadline bounds the request while upstream recovery is pending.
        // Only a published ownership decision can mark the Computer as owned.
        let decision = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            updates.wait_for(|current| current.computer.is_some()),
        )
        .await
        .map_err(|_| {
            TalosError::new(
                ErrorCode::Timeout,
                "Timed out restoring access to the local Computer.",
            )
        })?;
        Ok(
            decision.map_or(Some(LocalComputerDecision::Denied), |current| {
                current.computer
            }),
        )
    }

    /// Durable Computer imports wake registration reconciliation, never a read API.
    pub(crate) fn notify_refresh(&self, account_id: &str) {
        if let Some(refresh) = self
            .background
            .lock()
            .expect("Sync refresh tasks lock")
            .tasks
            .get(account_id)
        {
            refresh
                .state
                .send_modify(RefreshState::request_reconciliation);
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
                    .and_then(|refresh| refresh.state.borrow().actor.access_token.clone())
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
    host: std::sync::Weak<ApplicationSyncHost>,
    state: watch::Sender<RefreshState>,
    mut updates: watch::Receiver<RefreshState>,
    mut failures: watch::Receiver<Option<talos_error::ApiError>>,
    _lifetime: RefreshLifetime,
) {
    let mut wake_reason = RefreshWake::Started;
    loop {
        let Some(owner) = host.upgrade() else { break };
        let (actor, revision) = {
            let current = updates.borrow();
            (current.actor.clone(), current.revision)
        };
        let reconciliation = LocalComputerReconciliation {
            state: &state,
            revision,
        };
        state.send_if_modified(|current| {
            current.revision == revision && current.computer.take().is_some()
        });
        failures.borrow_and_update();
        let started = std::time::Instant::now();
        let mut phase = RefreshPhase::AwaitLifecycle;
        let result = {
            let _guard = owner.lifecycle.lock().await;
            async {
                owner
                    .refresh_account(&actor, &mut phase, &reconciliation)
                    .await?;
                phase = RefreshPhase::ObserveWorker;
                let local = talos_config::YamlConfigStore::new(owner.config.source_path())
                    .load_agent_node()?
                    .runner;
                Ok::<_, talos_error::TalosError>(match local {
                    Some(local) => owner
                        .sync_stores
                        .connections
                        .lock()
                        .await
                        .get(&local.node_id)
                        .filter(|(account, _)| account == &actor.user_id)
                        .and_then(|(_, connection)| connection.upgrade()),
                    None => None,
                })
            }
            .await
        };
        let failed = result.is_err();
        let should_retry = result.as_ref().is_err_and(retryable);
        let retry_after_seconds = should_retry.then_some(5_u64);
        let connection = match result {
            Err(error) => {
                let span = tracing::warn_span!("sync_account_initialization_failed", account_id = %actor.user_id, ?phase, ?wake_reason, elapsed_ms = started.elapsed().as_millis() as u64, retry_after_seconds);
                let _entered = span.enter();
                talos_logging::report_error(&error);
                reconciliation.complete(Err(&error));
                owner
                    .failures
                    .write()
                    .expect("sync failures lock")
                    .insert(actor.user_id.clone(), error.to_api_error());
                None
            }
            Ok(connection) => {
                owner
                    .failures
                    .write()
                    .expect("sync failures lock")
                    .remove(&actor.user_id);
                tracing::info!(account_id = %actor.user_id, ?wake_reason, elapsed_ms = started.elapsed().as_millis() as u64, "Sync account initialized; waiting for lifecycle changes");
                connection
            }
        };
        // The Hub runner owns H reconnect. Only this host's separate G connection
        // needs a lifecycle wake when its stream ends.
        drop(owner);
        if failed {
            wake_reason = tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)), if should_retry => RefreshWake::TemporaryFailureRetry,
                changed = updates.wait_for(|current| current.revision != revision) => {
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
            changed = updates.wait_for(|current| current.revision != revision) => {
                if changed.is_err() { break }
                RefreshWake::AccountOrComputerChanged
            },
            // Explicit credential recovery clears the registration error;
            // it wakes reconciliation just like a terminal failure.
            changed = failures.changed() => {
                if changed.is_err() { break }
                RefreshWake::WorkerStateChanged
            },
            _ = async {
                match &connection {
                    Some(connection) => connection.ended().await,
                    None => std::future::pending().await,
                }
            } => RefreshWake::WorkerConnectionEnded,
        };
    }
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
