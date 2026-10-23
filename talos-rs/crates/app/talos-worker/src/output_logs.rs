//! Node-local cleanup business logic; common guard-task owns its lifecycle.
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use talos_config::{Config, EffectiveWorkspace};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_guard_task::{GuardTask, GuardTaskContext, GuardTaskReason};
use talos_tools::runtimes::bash::{BashOutputMaintenance, OutputLogMaintenanceStats};

mod discovery;
const ROUND_BUDGET: Duration = Duration::from_secs(30);
const WORKSPACE_BUDGET: Duration = Duration::from_millis(250);
const CONTINUE_DELAY: Duration = Duration::from_secs(1);
const ACTIVE_WORKSPACES: usize = 8;
const WAITING_WORKSPACES: usize = 128;

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
    config: Arc<RwLock<Config>>,
}
impl OutputLogMaintenance {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        Self { config }
    }
}
struct Active {
    workspace: EffectiveWorkspace,
    scan: BashOutputMaintenance,
    failed: bool,
}
impl GuardTask for OutputLogMaintenance {
    async fn run(&self, context: GuardTaskContext) -> TalosResult<()> {
        run_round(self.config.clone(), context, ROUND_BUDGET, WORKSPACE_BUDGET).await
    }
}

async fn run_round(
    config: Arc<RwLock<Config>>,
    context: GuardTaskContext,
    round_budget: Duration,
    workspace_budget: Duration,
) -> TalosResult<()> {
    let initial = Arc::new(config.read().expect("Worker config lock poisoned").clone());
    let reason = match context.reason() {
        GuardTaskReason::Startup => "startup",
        GuardTaskReason::Periodic => "periodic",
        GuardTaskReason::Requested => "config_changed",
        GuardTaskReason::Restart => "restart",
    };
    let started = Instant::now();
    let policy = &initial.agent.kernel.tools.bash.output_log;
    tracing::info!(target: "talos::output_logs", reason,
        retention_days = policy.retention_days, max_workspace_bytes = policy.max_workspace_bytes,
        max_file_bytes = policy.max_file_bytes, interval_secs = policy.cleanup_interval_secs,
        "output log maintenance run started");
    let mut discovery = Some(discovery::Discovery::new());
    let mut waiting = VecDeque::new();
    let mut active: VecDeque<Active> = VecDeque::new();
    let mut stats = OutputLogMaintenanceStats::default();
    let mut failed = 0;
    let mut workspaces = 0;
    let mut batches = 0;
    let mut yields = 0;
    let mut tick = Instant::now();
    while !context.is_cancelled() {
        if tick.elapsed() >= round_budget {
            yields += 1;
            tracing::info!(target: "talos::output_logs", reason, batches, yields,
                active_workspaces = active.len(), waiting_workspaces = waiting.len(),
                "output log maintenance yielded; continuing the same round");
            tokio::select! { _ = context.cancelled() => break,
            _ = tokio::time::sleep(CONTINUE_DELAY) => {} }
            tick = Instant::now();
        }
        let remaining = round_budget.saturating_sub(tick.elapsed());
        if let Some(mut discover) = discovery.take() {
            if waiting.len() < WAITING_WORKSPACES {
                let (snapshot, cancel) = (initial.clone(), context.clone());
                let capacity = WAITING_WORKSPACES - waiting.len();
                let (next, found, errors, complete) = tokio::task::spawn_blocking(move || {
                    let (found, errors, complete) = discover.step(
                        &snapshot,
                        || cancel.is_cancelled(),
                        remaining.min(Duration::from_millis(50)),
                        capacity,
                    );
                    (discover, found, errors, complete)
                })
                .await
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Output Workspace discovery task failed.",
                        error,
                    )
                })?;
                waiting.extend(found);
                for error in errors {
                    failed += 1;
                    talos_logging::report_error(&error);
                }
                if !complete {
                    discovery = Some(next);
                }
            } else {
                discovery = Some(discover);
            }
        }
        while active.len() < ACTIVE_WORKSPACES {
            let Some(workspace) = waiting.pop_front() else {
                break;
            };
            active.push_back(Active {
                scan: BashOutputMaintenance::new(workspace.root.clone()),
                workspace,
                failed: false,
            });
            workspaces += 1;
        }
        if active.is_empty() {
            if discovery.is_none() && waiting.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
            continue;
        }
        let remaining = round_budget.saturating_sub(tick.elapsed());
        if remaining.is_zero() {
            continue;
        }
        let entry = active.pop_front().expect("active Workspace exists");
        let policy = {
            let current = config.read().expect("Worker config lock poisoned");
            if !current
                .resolve_workspace(&entry.workspace.workspace_id, &entry.workspace.user_id)
                .is_ok_and(|workspace| workspace.root == entry.workspace.root)
            {
                continue;
            }
            current.agent.kernel.tools.bash.output_log.clone()
        };
        let result = entry
            .scan
            .advance(policy, remaining.min(workspace_budget))
            .await;
        batches += 1;
        match result {
            Ok((scan, step)) => {
                tracing::debug!(target: "talos::output_logs", user_id = %entry.workspace.user_id,
                    scanned_entries = step.stats.scanned_entries, complete = step.complete,
                    "output log maintenance batch completed");
                if step.stats.scanned_entries == 0 && !step.complete && step.errors.is_empty() {
                    tokio::select! { _ = context.cancelled() => {},
                    _ = tokio::time::sleep(Duration::from_millis(5)) => {} }
                }
                stats.merge(&step.stats);
                let mut entry = Active {
                    scan,
                    workspace: entry.workspace,
                    failed: entry.failed,
                };
                for error in step.errors {
                    if !entry.failed {
                        failed += 1;
                        entry.failed = true;
                    }
                    report_workspace(&error, &entry.workspace);
                }
                if step.accounting_deferred {
                    tracing::info!(target: "talos::output_logs", user_id = %entry.workspace.user_id,
                        "output log accounting reconciliation deferred; live reservations retained");
                }
                if !step.complete {
                    active.push_back(entry);
                }
            }
            Err(error) => {
                if !entry.failed {
                    failed += 1;
                }
                report_workspace(&error, &entry.workspace);
            }
        }
        tokio::task::yield_now().await;
    }
    tracing::info!(target: "talos::output_logs", reason, workspaces, batches, yields,
        scanned_entries = stats.scanned_entries, deleted_logs = stats.deleted_logs,
        reclaimed_bytes = stats.reclaimed_bytes, recovered_logs = stats.recovered_logs,
        active_logs = stats.active_logs, failed_workspaces = failed, cancelled = context.is_cancelled(),
        elapsed_ms = started.elapsed().as_millis() as u64, "output log maintenance run completed");
    Ok(())
}

fn report_workspace(error: &TalosError, workspace: &EffectiveWorkspace) {
    let _span =
        tracing::error_span!("output_log_workspace", workspace_id = %workspace.workspace_id,
        user_id = %workspace.user_id, phase = "maintenance")
        .entered();
    talos_logging::report_error(error);
}

#[cfg(test)]
#[path = "output_logs_tests.rs"]
mod tests;

#[cfg(test)]
mod round_tests;
