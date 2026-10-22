//! Account-owned import/recovery observers. Remote recovery is polled alongside imports.
use super::application_sync::ApplicationSyncHost;
use std::collections::HashMap;
use tokio::task::JoinHandle;

#[derive(Default)]
pub(crate) struct SyncObservers {
    tasks: HashMap<String, JoinHandle<()>>,
}

struct ObserverLifetime(String);

impl Drop for ObserverLifetime {
    fn drop(&mut self) {
        tracing::info!(account_id = %self.0, "Sync import observer stopped");
    }
}

impl Drop for SyncObservers {
    fn drop(&mut self) {
        for task in self.tasks.values() {
            task.abort();
        }
    }
}

impl SyncObservers {
    pub(crate) fn remove(&mut self, account_id: &str) -> Option<JoinHandle<()>> {
        let task = self.tasks.remove(account_id);
        if let Some(task) = &task {
            task.abort();
        }
        task
    }
    pub(crate) fn drain(&mut self) -> Vec<JoinHandle<()>> {
        self.tasks
            .drain()
            .map(|(_, task)| {
                task.abort();
                task
            })
            .collect()
    }
}

impl ApplicationSyncHost {
    /// Called under the handle registry lock, exactly once per installed handle.
    pub(crate) fn observe_sync(&self, account_id: &str, handle: &talos_sync::SyncHandle) {
        let mut changes = handle.subscribe_changes();
        let mut status = handle.subscribe();
        let service = self.service.clone();
        let account_id = account_id.to_owned();
        let observed_account = account_id.clone();
        let events = self.event_service.clone();
        let sessions = self.sync_stores.sessions.clone();
        let host = self.self_weak.clone();
        tracing::info!(%account_id, "Sync import observer started");
        let lifetime = ObserverLifetime(account_id.clone());
        let task = tokio::spawn(async move {
            let _lifetime = lifetime;
            let mut was_connected = false;
            let mut recovery: Option<futures_util::future::BoxFuture<'static, ()>> = None;
            loop {
                tokio::select! {
                    summary = changes.recv() => {
                        let summary = match summary {
                            Ok(summary) => summary,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                if let Some(host) = host.upgrade() { host.notify_refresh(&account_id); }
                                if let Err(error) = super::application_sync::publish_account_changes(&sessions, &events, &account_id).await {
                                    talos_logging::report_error(&error);
                                }
                                continue;
                            }
                        };
                        match summary.resource {
                            talos_sync::SyncResourceKey::Session { session_id } => {
                                if let Err(error) = events.publish_imported(&session_id).await { talos_logging::report_error(&error); }
                            },
                            talos_sync::SyncResourceKey::Computer { computer_id } => {
                                if let Some(host) = host.upgrade() {
                                    match talos_config::YamlConfigStore::new(host.config.source_path()).load_agent_node() {
                                        Ok(node) if node.runner.as_ref().is_some_and(|runner| runner.node_id == computer_id) => host.notify_refresh(&account_id),
                                        Ok(_) => {},
                                        Err(error) => talos_logging::report_error(&error),
                                    }
                                }
                            },
                            talos_sync::SyncResourceKey::Project { .. } | talos_sync::SyncResourceKey::File { .. } => {},
                        }
                    },
                    changed = status.changed() => {
                        if changed.is_err() { break; }
                        let connected = status.borrow_and_update().connection == talos_sync::SyncConnectionState::Connected;
                        let reconnected = connected && !was_connected;
                        was_connected = connected;
                        if reconnected {
                            let service = service.clone();
                            let account_id = account_id.clone();
                            recovery = Some(Box::pin(async move {
                                if let Err(error) = service.retry_shared_session_heads(&account_id).await { talos_logging::report_error(&error); }
                            }));
                        } else if !connected {
                            recovery = None;
                        }
                    },
                    _ = async {
                        match recovery.as_mut() {
                            Some(recovery) => recovery.await,
                            None => std::future::pending().await,
                        }
                    } => { recovery = None; }
                }
            }
        });
        self.sync_stores
            .observers
            .lock()
            .expect("Sync observers lock")
            .tasks
            .insert(observed_account, task);
    }
}

pub(crate) async fn join_observer(task: JoinHandle<()>) {
    if let Err(error) = task.await {
        if !error.is_cancelled() {
            talos_logging::report_error(&talos_error::TalosError::internal_with_source(
                "The synchronization observer task failed.",
                error,
            ));
        }
    }
}

#[cfg(test)]
#[path = "application_sync_observer_tests.rs"]
mod tests;
