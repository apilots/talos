use talos_config::{Config, TalosPaths};
use talos_error::TalosResult;
use talos_logging::{LogGuard, StartupLoggingOptions};
use talos_upgrade::{BeforeConfigLoadReport, RuntimeResourcesReport};
use tracing::{error, info};

use super::TalosServerOptions;

/// Startup work completed after strict runtime configuration is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishStartupReport {
    pub resources: RuntimeResourcesReport,
}

/// Initializes logging and prepares resources needed for config loading.
pub fn prepare_startup(options: Option<&TalosServerOptions>) -> TalosResult<LogGuard> {
    let paths = TalosPaths::resolve();
    let stdout = options
        .and_then(|options| options.logging_stdout)
        .unwrap_or(true);
    let log_guard = talos_logging::init_startup_once(StartupLoggingOptions {
        config_file: paths.config_file.clone(),
        log_file: paths.log_file.clone(),
        stdout,
    })?;
    let report = prepare_before_config_load()?;
    info!(report = ?report, "pre-config resource preparation completed");
    Ok(log_guard)
}

/// Runs startup steps that require a validated runtime configuration.
pub fn finish_startup(config: &Config) -> TalosResult<FinishStartupReport> {
    let resources = prepare_runtime_resources(config)?;
    let report = FinishStartupReport { resources };
    info!(report = ?report, "finish_startup completed");
    Ok(report)
}

/// Prepares resources needed before runtime configuration is loaded.
fn prepare_before_config_load() -> TalosResult<BeforeConfigLoadReport> {
    talos_upgrade::prepare_before_config_load().map_err(|error| {
        error!("pre-config resource preparation failed: {}", error);
        error
    })
}

/// Prepares resources that require a validated runtime configuration.
fn prepare_runtime_resources(config: &Config) -> TalosResult<RuntimeResourcesReport> {
    talos_upgrade::prepare_runtime_resources(config).map_err(|error| {
        error!("runtime resource preparation failed: {}", error);
        error
    })
}
