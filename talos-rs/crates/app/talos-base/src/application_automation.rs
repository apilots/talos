use std::sync::Arc;
use std::time::Duration;

use talos_automation::{Automation, AutomationJob, AutomationJobControl};
use talos_config::{DreamMemoryMaintenanceConfig, ProfileUsageRetentionConfig, TalosTimezone};
use talos_coordinator::AgentCoordinator;
#[cfg(feature = "server")]
use talos_coordinator::CoordinatorImpl;
use talos_error::TalosResult;
#[cfg(feature = "server")]
use talos_gateway_http::{ServerLifecycle, ServerLifecycleState};
use talos_service::ProfileUsageRetention;
use talos_store::profile::ProfileStore;
#[cfg(feature = "server")]
use tokio::signal;
use tracing::error;
#[cfg(feature = "server")]
use tracing::{info, warn};

use super::dream_memory_maintenance::DreamMemoryMaintenance;
use super::TalosApplicationRuntime;

const BACKGROUND_TASK_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(feature = "server")]
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
        self.service.begin_execution_drain();
        let execution_result = self
            .worker
            .stop_execution(BACKGROUND_TASK_DRAIN_TIMEOUT)
            .await;
        // Cursor recovery still needs the registered Harness until delivery finishes.
        let delivery_result =
            drain_product_delivery(&self.service, BACKGROUND_TASK_DRAIN_TIMEOUT).await;
        let registration_result = match self.worker_registration.lock().await.take() {
            Some(registration) => registration.shutdown().await,
            None => Ok(()),
        };
        let background_result = self.worker.shutdown_background_tasks().await;
        let drain_result = self
            .coordinator
            .shutdown(BACKGROUND_TASK_DRAIN_TIMEOUT)
            .await;
        let result = match (automation_result, drain_result) {
            (Ok(()), result) => result,
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(drain_error)) => {
                error!(error = %drain_error, "background task drain failed after automation shutdown error");
                Err(error)
            }
        };
        let result = combine(result, execution_result);
        let result = combine(result, delivery_result);
        let result = combine(result, background_result);
        let result = combine(result, registration_result);
        let result = combine(result, self.worker.browser().shutdown().await);
        let result = combine(result, self.worker.plugin_manager().shutdown_mcp().await);
        let result = combine(result, self.worker_rpc.shutdown().await);
        {
            self.resource_stop.cancel();
            result
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
    initial_paused: Vec<talos_types::BuiltinJobId>,
    loop_service: Option<std::sync::Weak<talos_service::TalosService>>,
) -> TalosResult<Automation> {
    let retention = ProfileUsageRetention::new(profiles, profile_config.clone());
    let mut jobs = vec![
        AutomationJob::cron_on_start(
            "profile-usage-retention",
            profile_config.cron,
            timezone,
            move || {
                let retention = retention.clone();
                async move { retention.run().await.map(|_| ()) }
            },
        )
        .with_control(AutomationJobControl {
            paused: initial_paused.contains(&talos_types::BuiltinJobId::ProfileUsageRetention),
            run_once: true,
        }),
        AutomationJob::cron(
            "dream-memory-maintenance",
            memory_config.cron,
            timezone,
            move || {
                let memory = memory.clone();
                async move { memory.run().await.map(|_| ()) }
            },
        )
        .with_control(AutomationJobControl {
            paused: initial_paused.contains(&talos_types::BuiltinJobId::DreamMemoryMaintenance),
            run_once: true,
        }),
    ];
    if let Some(service) = loop_service {
        jobs.push(AutomationJob::cron(
            "session-loop-scan",
            "*/1 * * * * *",
            timezone,
            move || {
                let service = service.clone();
                async move {
                    if let Some(service) = service.upgrade() {
                        service.reconcile_worker_events().await?;
                        service
                            .scan_session_loops(chrono::Utc::now().timestamp())
                            .await?;
                    }
                    Ok(())
                }
            },
        ));
    }
    Automation::start(jobs).await
}

pub(super) async fn drain_product_delivery(
    service: &talos_service::TalosService,
    timeout: Duration,
) -> TalosResult<()> {
    tokio::time::timeout(timeout, service.drain_execution_delivery())
        .await
        .map_err(|error| {
            talos_error::TalosError::internal_with_source(
                "Product execution delivery did not finish during shutdown.",
                error,
            )
        })
}

pub(super) fn combine(
    result: TalosResult<()>,
    shutdown_result: TalosResult<()>,
) -> TalosResult<()> {
    match (result, shutdown_result) {
        (Ok(()), result) => result,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(shutdown_error)) => {
            error!(error = %shutdown_error, "component shutdown failed");
            Err(error)
        }
    }
}

#[cfg(feature = "server")]
pub(super) async fn shutdown_server(
    worker: &talos_worker::TalosWorker,
    coordinator: &Arc<CoordinatorImpl>,
    automation: &Automation,
    service: &talos_service::TalosService,
    lifecycle: &Arc<ServerLifecycle>,
    plugins: &talos_service::PluginManagerService,
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
    let result = automation.shutdown().await;
    service.begin_execution_drain();
    let result = combine(
        result,
        worker.stop_execution(BACKGROUND_TASK_DRAIN_TIMEOUT).await,
    );
    let result = combine(
        result,
        drain_product_delivery(service, BACKGROUND_TASK_DRAIN_TIMEOUT).await,
    );
    if let Err(err) = coordinator.shutdown(BACKGROUND_TASK_DRAIN_TIMEOUT).await {
        warn!(error = %err, "background task drain failed on shutdown");
    }
    let result = combine(result, plugins.shutdown_mcp().await);
    lifecycle.store(ServerLifecycleState::Stopped);
    result
}
