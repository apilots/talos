//! Account maintenance is owned by the host, never awaited by snapshot queries.
use super::application_sync::ApplicationSyncHost;
use std::collections::HashMap;
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
        tracing::info!(%account_id, "Sync account maintenance started");
        let lifetime = RefreshLifetime(account_id.clone());
        let task = tokio::spawn(maintain_account(host, updates, failures, lifetime));
        background
            .tasks
            .insert(account_id, RefreshTask { task, actor });
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

    pub(crate) fn refreshes_closed(&self) -> bool {
        self.background
            .lock()
            .expect("Sync refresh tasks lock")
            .closed
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
    host: std::sync::Weak<ApplicationSyncHost>,
    mut updates: tokio::sync::watch::Receiver<Actor>,
    mut failures: tokio::sync::watch::Receiver<Option<talos_error::ApiError>>,
    _lifetime: RefreshLifetime,
) {
    loop {
        let Some(owner) = host.upgrade() else { break };
        let actor = updates.borrow_and_update().clone();
        failures.borrow_and_update();
        let result = {
            let _guard = owner.lifecycle.lock().await;
            async {
                owner.refresh_account(&actor).await?;
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
        let connection = match result {
            Err(error) => {
                let span = tracing::warn_span!("sync_account_initialization_failed", account_id = %actor.user_id, retry_after_seconds = 5);
                let _entered = span.enter();
                talos_logging::report_error(&error);
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
                tracing::info!(account_id = %actor.user_id, "Sync account initialized; waiting for lifecycle changes");
                connection
            }
        };
        // The Hub runner owns H reconnect. Only this host's separate G connection
        // needs a lifecycle wake when its stream ends.
        drop(owner);
        if failed {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {},
                changed = updates.changed() => if changed.is_err() { break },
            }
            continue;
        }
        tokio::select! {
            changed = updates.changed() => if changed.is_err() { break },
            // Explicit credential recovery clears the registration error;
            // it wakes reconciliation just like a terminal failure.
            changed = failures.changed() => if changed.is_err() { break },
            _ = async {
                match &connection {
                    Some(connection) => connection.ended().await,
                    None => std::future::pending().await,
                }
            } => {},
        }
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
