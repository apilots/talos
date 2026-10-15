use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::StorageTransaction;
use tokio::{
    sync::{broadcast, mpsc, watch, Mutex},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    SyncChange, SyncConnectionState, SyncExportParams, SyncImportBatch, SyncScanPage,
    SyncScanParams, SyncStartParams, SyncStatus, TalosSync,
};

mod control;
mod operations;
mod presence;
mod transport;

use control::ControlCommand;

struct RunnerChannels {
    presence_computers: watch::Receiver<Vec<String>>,
    upload_cursor: i64,
    upload_cut: i64,
    failed_uploads: HashMap<crate::SyncResourceKey, (Option<i64>, talos_error::ApiError)>,
    wake: mpsc::Receiver<()>,
    commands: mpsc::Receiver<ControlCommand>,
    status: watch::Sender<SyncStatus>,
    changes: broadcast::Sender<talos_sync_types::SyncResourceSummary>,
}

/// Existing Services/Stores supply authorized facts; import has no execution or
/// accounting side effects. Every apply uses the same transaction as sync state.
pub trait SyncDataAdapter: Send + Sync {
    fn scan(
        &self,
        params: SyncScanParams,
    ) -> impl Future<Output = TalosResult<SyncScanPage>> + Send;
    /// Export one original business read cut while holding the caller's transaction.
    fn export(
        &self,
        tx: &mut dyn StorageTransaction,
        params: SyncExportParams,
    ) -> impl Future<Output = TalosResult<Vec<SyncChange>>> + Send;
    /// Return only absent immutable dependencies. The runner recovers them after
    /// rolling back its candidate, never while holding the business transaction.
    fn missing_dependencies(
        &self,
        _tx: &mut dyn StorageTransaction,
        _batch: &SyncImportBatch,
    ) -> impl Future<Output = TalosResult<Vec<crate::SyncResourceKey>>> + Send {
        async { Ok(Vec::new()) }
    }
    fn apply(
        &self,
        tx: &mut dyn StorageTransaction,
        batch: &SyncImportBatch,
    ) -> impl Future<Output = TalosResult<()>> + Send;
}

/// Owns only synchronization transport. Dropping it never shuts down a Worker.
pub struct SyncHandle {
    presence_computers: watch::Sender<Vec<String>>,
    sync: Arc<TalosSync>,
    binding: crate::SyncBinding,
    stop: CancellationToken,
    wake: mpsc::Sender<()>,
    status: watch::Receiver<SyncStatus>,
    commands: mpsc::Sender<ControlCommand>,
    changes: broadcast::Sender<talos_sync_types::SyncResourceSummary>,
    task: Mutex<Option<JoinHandle<TalosResult<()>>>>,
}

impl SyncHandle {
    /// Refresh only ephemeral observations after the host verifies local ownership.
    /// This never changes durable capture aliases or grants execution authority.
    pub fn set_presence_computers(&self, computers: Vec<String>) -> TalosResult<()> {
        if computers
            .iter()
            .any(|id| uuid::Uuid::parse_str(id).is_err())
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid Computer presence identity.",
            ));
        }
        self.presence_computers.send_replace(computers);
        Ok(())
    }
    /// Coalesce committed batches into one bounded wakeup, without network waits.
    pub fn notify_committed(&self) {
        let _ = self.wake.try_send(());
    }
    pub fn status(&self) -> SyncStatus {
        self.status.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<SyncStatus> {
        self.status.clone()
    }

    /// Bounded discovery notifications. A lagged reader reloads original APIs;
    /// these summaries never replace the durable Timeline or product Stores.
    pub fn subscribe_changes(&self) -> broadcast::Receiver<talos_sync_types::SyncResourceSummary> {
        self.changes.subscribe()
    }

    pub async fn shutdown(&self) -> TalosResult<()> {
        self.stop.cancel();
        let Some(mut task) = self.task.lock().await.take() else {
            return Ok(());
        };
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(result) => result.map_err(|error| {
                TalosError::internal_with_source(
                    "Join synchronization runner; technical details were withheld.",
                    error,
                )
            })?,
            Err(_) => {
                task.abort();
                match task.await {
                    Ok(result) => result?,
                    Err(error) if error.is_cancelled() => {}
                    Err(error) => {
                        return Err(TalosError::internal_with_source(
                            "Stop synchronization runner; technical details were withheld.",
                            error,
                        ))
                    }
                }
                Err(TalosError::new(
                    ErrorCode::Timeout,
                    "Synchronization shutdown timed out.",
                ))
            }
        }
    }
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl TalosSync {
    /// Start one verified account binding using the shared SDK. The adapter is
    /// the only business bridge; authentication uses the host's original callback.
    pub fn start<A: SyncDataAdapter + 'static>(
        self: Arc<Self>,
        adapter: Arc<A>,
        params: SyncStartParams,
    ) -> TalosResult<SyncHandle> {
        if params.reconnect_delay.is_zero() || params.binding.peer_id != self.peer_id() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid synchronization runner configuration.",
            ));
        }
        let stop = CancellationToken::new();
        let (wake, receiver) = mpsc::channel(1);
        let (commands, received_commands) = mpsc::channel(8);
        let (changes, _) = broadcast::channel(32);
        let (status, observed) = watch::channel(SyncStatus {
            connection: SyncConnectionState::Connecting,
            pending_changes: 0,
            last_acknowledged_at: None,
            last_error: None,
        });
        let (presence_computers, observed_computers) = watch::channel(
            params
                .binding
                .computers
                .iter()
                .map(|alias| alias.computer_id.clone())
                .collect(),
        );
        let channels = RunnerChannels {
            presence_computers: observed_computers,
            upload_cursor: 0,
            upload_cut: 0,
            failed_uploads: HashMap::new(),
            wake: receiver,
            commands: received_commands,
            status,
            changes: changes.clone(),
        };
        let sync = self.clone();
        let binding = params.binding.clone();
        let task = tokio::spawn(run(self, adapter, params, stop.clone(), channels));
        Ok(SyncHandle {
            presence_computers,
            sync,
            binding,
            stop,
            wake,
            status: observed,
            commands,
            changes,
            task: Mutex::new(Some(task)),
        })
    }
}

async fn run<A: SyncDataAdapter>(
    sync: Arc<TalosSync>,
    adapter: Arc<A>,
    params: SyncStartParams,
    stop: CancellationToken,
    mut channels: RunnerChannels,
) -> TalosResult<()> {
    loop {
        let enabled = match sync.enabled(&params.binding.scope.account_id).await {
            Ok(enabled) => enabled,
            Err(error) => {
                channels.status.send_modify(|value| {
                    value.connection = SyncConnectionState::Disabled;
                    value.last_error = Some(error.to_api_error());
                });
                return Err(error);
            }
        };
        if !enabled {
            break;
        }
        channels.status.send_modify(|value| {
            value.connection = SyncConnectionState::Connecting;
        });
        let result = transport::connected(&sync, &*adapter, &params, &stop, &mut channels).await;
        if stop.is_cancelled() {
            break;
        }
        if let Err(error) = result {
            channels.status.send_modify(|value| {
                value.connection = SyncConnectionState::Disconnected;
                value.last_error = Some(error.to_api_error());
            });
        }
        while let Ok(command) = channels.commands.try_recv() {
            command.reject();
        }
        tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(params.reconnect_delay) => {} }
    }
    channels
        .status
        .send_modify(|value| value.connection = SyncConnectionState::Disabled);
    Ok(())
}
