use std::sync::Arc;
use std::time::Duration;

use talos_automation::{Automation, AutomationJob, AutomationJobControl};
use talos_config::{DreamMemoryMaintenanceConfig, ProfileUsageRetentionConfig, TalosTimezone};
use talos_coordinator::{AgentCoordinator, CoordinatorImpl};
use talos_error::TalosResult;
use talos_gateway_http::{ServerLifecycle, ServerLifecycleState};
use talos_service::ProfileUsageRetention;
use talos_store::profile::ProfileStore;
use tokio::signal;
use tracing::{error, info, warn};

use super::application_computer_maintenance::{ComputerJobTarget, ComputerMaintenance};
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
        let registration_result = match self.worker_registration.lock().await.take() {
            Some(registration) => registration.shutdown().await,
            None => Ok(()),
        };
        let automation_result = self.automation.shutdown().await;
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
        let result = combine(result, registration_result);
        let result = combine(result, self.worker.browser().shutdown().await);
        let result = combine(result, self.worker.plugin_manager().shutdown_mcp().await);
        let result = combine(result, self.worker_rpc.shutdown().await);
        combine(result, self.resource_rpc.shutdown().await)
    }
}

#[derive(Clone)]
pub(super) enum MaintenanceTarget {
    Application,
    Computer(ComputerJobTarget),
    Unassigned(std::sync::Weak<ComputerMaintenance>),
}

impl MaintenanceTarget {
    fn job_name(&self, id: talos_types::BuiltinJobId) -> String {
        match self {
            Self::Application => id.as_str().to_owned(),
            Self::Computer(target) => target.job_name(id),
            Self::Unassigned(_) => format!("unassigned/{}", id.as_str()),
        }
    }
}

#[derive(Clone)]
pub(super) struct MaintenanceJobs {
    retention: ProfileUsageRetention,
    profile_config: ProfileUsageRetentionConfig,
    memory: DreamMemoryMaintenance,
    memory_config: DreamMemoryMaintenanceConfig,
    pub timezone: TalosTimezone,
}

impl MaintenanceJobs {
    pub(super) fn new(
        profiles: Arc<dyn ProfileStore>,
        profile_config: ProfileUsageRetentionConfig,
        memory: DreamMemoryMaintenance,
        memory_config: DreamMemoryMaintenanceConfig,
        timezone: TalosTimezone,
    ) -> Self {
        Self {
            retention: ProfileUsageRetention::new(profiles, profile_config.clone()),
            profile_config,
            memory,
            memory_config,
            timezone,
        }
    }

    pub(super) fn definitions(
        &self,
        initial_paused: &[talos_types::BuiltinJobId],
        target: MaintenanceTarget,
    ) -> Vec<AutomationJob> {
        use talos_types::BuiltinJobId;
        let retention_name = target.job_name(BuiltinJobId::ProfileUsageRetention);
        let memory_name = target.job_name(BuiltinJobId::DreamMemoryMaintenance);
        let retention_target = target.clone();
        let memory_target = target.clone();
        let retention = self.retention.clone();
        let memory = self.memory.clone();
        let retention = AutomationJob::cron_on_start(
            retention_name,
            self.profile_config.cron.clone(),
            self.timezone,
            move || {
                let retention = retention.clone();
                let target = retention_target.clone();
                async move {
                    match target {
                        MaintenanceTarget::Computer(target) => {
                            if let Some(scope) = target.current_scope().await? {
                                retention.run_for_computer(&scope).await?;
                            }
                        }
                        MaintenanceTarget::Application => {
                            retention.run().await?;
                        }
                        MaintenanceTarget::Unassigned(owner) => {
                            if let Some(owner) = owner.upgrade() {
                                retention
                                    .run_unassigned(&owner.unassigned_data().await?)
                                    .await?;
                            }
                        }
                    }
                    Ok(())
                }
            },
        );
        let memory = AutomationJob::cron(
            memory_name,
            self.memory_config.cron.clone(),
            self.timezone,
            move || {
                let memory = memory.clone();
                let target = memory_target.clone();
                async move {
                    match target {
                        MaintenanceTarget::Computer(target) => {
                            if let Some(scope) = target.current_scope().await? {
                                memory.run_for_computer(&scope).await?;
                            }
                        }
                        MaintenanceTarget::Application => {
                            memory.run().await?;
                        }
                        MaintenanceTarget::Unassigned(owner) => {
                            if let Some(owner) = owner.upgrade() {
                                memory
                                    .run_unassigned(owner.unassigned_data().await?)
                                    .await?;
                            }
                        }
                    }
                    Ok(())
                }
            },
        );
        [
            (BuiltinJobId::ProfileUsageRetention, retention),
            (BuiltinJobId::DreamMemoryMaintenance, memory),
        ]
        .into_iter()
        .filter_map(|(id, job)| {
            let paused = initial_paused.contains(&id);
            if matches!(target, MaintenanceTarget::Unassigned(_)) {
                // Honor existing application-level pause state for these legacy records.
                (!paused).then_some(job)
            } else {
                Some(job.with_control(AutomationJobControl {
                    paused,
                    run_once: true,
                }))
            }
        })
        .collect()
    }
}

/// Starts maintenance and the existing session scan on one scheduler.
pub(super) async fn start_maintenance(
    jobs: &MaintenanceJobs,
    initial_paused: Vec<talos_types::BuiltinJobId>,
    computers: Option<&Arc<ComputerMaintenance>>,
    loop_service: Option<std::sync::Weak<talos_service::TalosService>>,
) -> TalosResult<Automation> {
    let mut definitions = match computers {
        Some(computers) => {
            let mut definitions = jobs.definitions(
                &initial_paused,
                MaintenanceTarget::Unassigned(Arc::downgrade(computers)),
            );
            definitions.push(computers.refresh_job(jobs.timezone));
            definitions
        }
        None => jobs.definitions(&initial_paused, MaintenanceTarget::Application),
    };
    if let Some(service) = loop_service {
        definitions.push(AutomationJob::cron(
            "session-loop-scan",
            "*/1 * * * * *",
            jobs.timezone,
            move || {
                let service = service.clone();
                async move {
                    if let Some(service) = service.upgrade() {
                        service
                            .scan_session_loops(chrono::Utc::now().timestamp())
                            .await?;
                    }
                    Ok(())
                }
            },
        ));
    }
    Automation::start(definitions).await
}

pub(super) async fn finish(
    result: TalosResult<()>,
    automation: &Automation,
    plugins: &talos_service::PluginManagerService,
) -> TalosResult<()> {
    let result = combine(result, automation.shutdown().await);
    combine(result, plugins.shutdown_mcp().await)
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

pub(super) async fn shutdown_server(
    coordinator: &Arc<CoordinatorImpl>,
    automation: &Automation,
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
    if let Err(err) = coordinator.shutdown(BACKGROUND_TASK_DRAIN_TIMEOUT).await {
        warn!(error = %err, "background task drain failed on shutdown");
    }
    let result = combine(automation.shutdown().await, plugins.shutdown_mcp().await);
    lifecycle.store(ServerLifecycleState::Stopped);
    result
}
