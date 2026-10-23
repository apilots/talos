//! Node-local output retention belongs to the execution runtime, not routing.
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use talos_config::{Config, EffectiveWorkspace};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_tools::runtimes::bash::maintain_bash_output_workspace;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(crate) fn validate_policy(
    config: &Config,
    kernel: &talos_config::AgentKernelConfig,
) -> TalosResult<()> {
    if kernel.tools.bash.output_log != config.agent.kernel.tools.bash.output_log {
        return Err(TalosError::new(ErrorCode::InvalidRequest,
            "Bash output retention and quotas must use the execution node policy shared by all Agents."));
    }
    Ok(())
}

pub(crate) struct OutputLogMaintenance {
    stop: CancellationToken,
    changed: Arc<Notify>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl OutputLogMaintenance {
    pub fn start(config: Arc<RwLock<Config>>, changed: Arc<Notify>) -> Arc<Self> {
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let task_changed = changed.clone();
        let task = tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                if task_stop.is_cancelled() {
                    break;
                }
                let attempt = Instant::now();
                match tokio::spawn(run(config.clone(), task_stop.clone(), task_changed.clone()))
                    .await
                {
                    Ok(()) => break,
                    Err(error) => talos_logging::report_error(&TalosError::internal_with_source(
                        "Output log maintenance task exited unexpectedly.",
                        error,
                    )),
                }
                if attempt.elapsed() >= Duration::from_secs(60) {
                    backoff = Duration::from_secs(1);
                }
                tracing::warn!(target: "talos::output_logs", retry_after_secs = backoff.as_secs(),
                    "output log maintenance restarting after failure");
                tokio::select! {
                    biased;
                    _ = task_stop.cancelled() => break,
                    _ = tokio::time::sleep(backoff) => {},
                }
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
            tracing::info!(target: "talos::output_logs", reason = "shutdown",
                "output log maintenance supervisor stopped");
        });
        Arc::new(Self {
            stop,
            changed,
            task: Mutex::new(Some(task)),
        })
    }

    pub async fn shutdown(&self) -> TalosResult<()> {
        self.stop.cancel();
        // Hold until joined: concurrent Worker clones cannot report a completed
        // shutdown while another clone is still awaiting the filesystem batch.
        let mut task = self.task.lock().await;
        if let Some(running) = task.as_mut() {
            let result = running.await.map_err(|error| {
                TalosError::internal_with_source(
                    "Output log maintenance task exited unexpectedly.",
                    error,
                )
            });
            *task = None;
            result?;
        }
        Ok(())
    }
}

impl Drop for OutputLogMaintenance {
    fn drop(&mut self) {
        self.stop.cancel();
        self.changed.notify_one();
    }
}

async fn run(config: Arc<RwLock<Config>>, stop: CancellationToken, changed: Arc<Notify>) {
    let initial = config.read().expect("Worker config lock poisoned").clone();
    let policy = &initial.agent.kernel.tools.bash.output_log;
    tracing::info!(target: "talos::output_logs", interval_secs = policy.cleanup_interval_secs,
        retention_days = policy.retention_days, max_workspace_bytes = policy.max_workspace_bytes,
        "output log maintenance started");
    let mut reason = "startup";
    loop {
        if stop.is_cancelled() {
            break;
        }
        let current = config.read().expect("Worker config lock poisoned").clone();
        let interval = current
            .agent
            .kernel
            .tools
            .bash
            .output_log
            .cleanup_interval_secs;
        run_round(current, &stop, reason).await;
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            _ = changed.notified() => reason = "config_changed",
            _ = tokio::time::sleep(Duration::from_secs(interval)) => reason = "periodic",
        }
    }
    tracing::info!(target: "talos::output_logs", reason = "shutdown", "output log maintenance stopped");
}

async fn run_round(config: Config, stop: &CancellationToken, reason: &str) {
    let started = Instant::now();
    tracing::info!(target: "talos::output_logs", reason, "output log maintenance run started");
    let discover_config = config.clone();
    let discover_stop = stop.clone();
    let roots =
        tokio::task::spawn_blocking(move || discover(&discover_config, &discover_stop)).await;
    let roots = match roots {
        Ok(roots) => roots,
        Err(error) => {
            talos_logging::report_error(&TalosError::internal_with_source(
                "Discover output log Workspaces failed.",
                error,
            ));
            tracing::warn!(target: "talos::output_logs", elapsed_ms = started.elapsed().as_millis() as u64,
                failed_workspaces = 1, "output log maintenance run failed");
            return;
        }
    };
    let mut scanned = 0;
    let mut deleted = 0;
    let mut reclaimed = 0;
    let mut recovered = 0;
    let mut active = 0;
    let mut failed = roots.failed_prefixes;
    let mut workspaces = 0;
    for workspace in roots.workspaces {
        if stop.is_cancelled() {
            break;
        }
        workspaces += 1;
        match maintain_bash_output_workspace(
            workspace.root.clone(),
            config.agent.kernel.tools.bash.output_log.clone(),
        )
        .await
        {
            Ok(stats) => {
                scanned += stats.scanned_entries;
                deleted += stats.deleted_logs;
                reclaimed += stats.reclaimed_bytes;
                recovered += stats.recovered_logs;
                active += stats.active_logs;
            }
            Err(error) => {
                failed += 1;
                let _span = tracing::error_span!("output_log_workspace", workspace_id = %workspace.workspace_id,
                    user_id = %workspace.user_id, workspace_root = %workspace.root.display(), phase = "maintenance").entered();
                talos_logging::report_error(&error);
            }
        }
        tokio::task::yield_now().await;
    }
    tracing::info!(target: "talos::output_logs", reason, workspaces, scanned_entries = scanned,
        deleted_logs = deleted, reclaimed_bytes = reclaimed, recovered_logs = recovered, active_logs = active,
        failed_workspaces = failed, cancelled = stop.is_cancelled(), elapsed_ms = started.elapsed().as_millis() as u64,
        "output log maintenance run completed");
}

struct Discovery {
    workspaces: Vec<EffectiveWorkspace>,
    failed_prefixes: usize,
}

fn discover(config: &Config, stop: &CancellationToken) -> Discovery {
    let mut result = Discovery {
        workspaces: Vec::new(),
        failed_prefixes: 0,
    };
    for definition in &config.workspaces.definitions {
        if stop.is_cancelled() {
            break;
        }
        match discover_prefix(config, &definition.workspace_id, stop) {
            Ok(roots) => result.workspaces.extend(roots),
            Err(error) => {
                result.failed_prefixes += 1;
                let _span = tracing::error_span!("output_log_discovery", workspace_id = %definition.workspace_id, phase = "discovery").entered();
                talos_logging::report_error(&error);
            }
        }
    }
    result
}

fn discover_prefix(
    config: &Config,
    workspace_id: &str,
    stop: &CancellationToken,
) -> TalosResult<Vec<EffectiveWorkspace>> {
    let prefix = config.workspace_root_prefix(workspace_id)?;
    match std::fs::symlink_metadata(&prefix) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Output maintenance Workspace prefix must be a regular directory.",
            ))
        }
        Err(error) => {
            return Err(TalosError::internal_with_source(
                "Read output Workspace prefix failed.",
                error,
            ))
        }
    }
    let mut roots = Vec::new();
    for entry in std::fs::read_dir(&prefix).map_err(|error| {
        TalosError::internal_with_source("List output Workspaces failed.", error)
    })? {
        if stop.is_cancelled() {
            break;
        }
        let entry = entry.map_err(|error| {
            TalosError::internal_with_source("Read output Workspace entry failed.", error)
        })?;
        let kind = entry.file_type().map_err(|error| {
            TalosError::internal_with_source("Read output Workspace kind failed.", error)
        })?;
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let Some(user) = name.to_str() else {
            continue;
        };
        if talos_config::validate_user_workspace_segment(user).is_err() {
            continue;
        }
        let workspace = config.resolve_workspace(workspace_id, user)?;
        // Do not create log directories in Workspaces that never used spill.
        match workspace.root.join("tool-output").symlink_metadata() {
            Ok(_) => roots.push(workspace),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                let _span =
                    tracing::error_span!("output_log_discovery", user_id = user, phase = "entry")
                        .entered();
                talos_logging::report_error(&TalosError::internal_with_source(
                    "Read output log root failed.",
                    error,
                ));
            }
        }
    }
    Ok(roots)
}

#[cfg(test)]
#[path = "output_logs_tests.rs"]
mod tests;
