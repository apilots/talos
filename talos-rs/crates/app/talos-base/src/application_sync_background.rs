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
                refresh.actor.send_replace(actor.clone());
                return;
            }
        }
        let host = self.self_weak.clone();
        let account_id = actor.user_id.clone();
        let (actor, updates) = tokio::sync::watch::channel(actor.clone());
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(host) = host.upgrade() else { break };
                let actor = updates.borrow().clone();
                let _guard = host.lifecycle.lock().await;
                if let Err(error) = host.refresh_account(&actor).await {
                    talos_logging::report_error(&error);
                    host.failures
                        .write()
                        .expect("sync failures lock")
                        .insert(actor.user_id.clone(), error.to_api_error());
                } else {
                    host.failures
                        .write()
                        .expect("sync failures lock")
                        .remove(&actor.user_id);
                }
            }
        });
        background
            .tasks
            .insert(account_id, RefreshTask { task, actor });
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
