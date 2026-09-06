use std::sync::Arc;
use std::time::Duration;

use talos_automation::{Automation, AutomationJob};
use talos_config::{DreamMemoryMaintenanceConfig, ProfileUsageRetentionConfig, TalosTimezone};
use talos_coordinator::{AgentCoordinator, CoordinatorImpl};
use talos_error::TalosResult;
use talos_gateway_http::{ServerLifecycle, ServerLifecycleState};
use talos_service::ProfileUsageRetention;
use talos_store::profile::ProfileStore;
use tokio::signal;
use tracing::{error, info, warn};

use super::dream_memory_maintenance::DreamMemoryMaintenance;
use super::TalosApplicationRuntime;

const BACKGROUND_TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

impl TalosApplicationRuntime {
    pub async fn shutdown(&self) -> TalosResult<()> {
        let automation_result = self.automation.shutdown().await;
        let drain_result = self
            .coordinator
            .shutdown(BACKGROUND_TASK_DRAIN_TIMEOUT)
            .await;
        match (automation_result, drain_result) {
            (Ok(()), result) => result,
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(drain_error)) => {
                error!(error = %drain_error, "background task drain failed after automation shutdown error");
                Err(error)
            }
        }
    }
}

/// Starts application-owned maintenance jobs from validated configuration.
pub(super) async fn start_maintenance(
    profiles: Arc<dyn ProfileStore>,
    profile_config: ProfileUsageRetentionConfig,
    memory: DreamMemoryMaintenance,
    memory_config: DreamMemoryMaintenanceConfig,
    timezone: TalosTimezone,
) -> TalosResult<Automation> {
    let retention = ProfileUsageRetention::new(profiles, profile_config.clone());
    Automation::start(vec![
        AutomationJob::cron_on_start(
            "profile-usage-retention",
            profile_config.cron,
            timezone,
            move || {
                let retention = retention.clone();
                async move { retention.run().await.map(|_| ()) }
            },
        ),
        AutomationJob::cron(
            "dream-memory-maintenance",
            memory_config.cron,
            timezone,
            move || {
                let memory = memory.clone();
                async move { memory.run().await.map(|_| ()) }
            },
        ),
    ])
    .await
}

pub(super) async fn finish(result: TalosResult<()>, automation: &Automation) -> TalosResult<()> {
    combine(result, automation.shutdown().await)
}

pub(super) fn combine(
    result: TalosResult<()>,
    shutdown_result: TalosResult<()>,
) -> TalosResult<()> {
    match (result, shutdown_result) {
        (Ok(()), result) => result,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(shutdown_error)) => {
            error!(error = %shutdown_error, "automation shutdown failed");
            Err(error)
        }
    }
}

pub(super) async fn shutdown_server(
    coordinator: &Arc<CoordinatorImpl>,
    automation: &Automation,
    lifecycle: &Arc<ServerLifecycle>,
) -> TalosResult<()> {
    if lifecycle.current() == ServerLifecycleState::Stopped {
        return Ok(());
    }
    if !lifecycle.current().is_quiescing_or_later() {
        lifecycle.store(ServerLifecycleState::Quiescing);
    }
    lifecycle.store(ServerLifecycleState::Draining);

    info!(
        timeout_secs = BACKGROUND_TASK_DRAIN_TIMEOUT.as_secs(),
        "draining background tasks before shutdown"
    );
    if let Err(err) = coordinator.shutdown(BACKGROUND_TASK_DRAIN_TIMEOUT).await {
        warn!(error = %err, "background task drain failed on shutdown");
    }
    automation.shutdown().await?;
    lifecycle.store(ServerLifecycleState::Stopped);
    Ok(())
}
