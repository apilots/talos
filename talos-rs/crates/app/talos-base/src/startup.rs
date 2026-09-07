use talos_config::{Config, TalosPaths};
use talos_error::TalosResult;
use talos_logging::{LogGuard, StartupLoggingOptions};
use talos_upgrade::{BeforeConfigLoadReport, RuntimeResourcesReport};
use tracing::info;

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
    })
    .inspect_err(|error| {
        // No logging sink exists yet; stderr is the startup diagnostic boundary.
        eprintln!("startup logging failed: {error:#?}");
    })?;
    let report = prepare_before_config_load().inspect_err(talos_logging::report_error)?;
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
    talos_upgrade::prepare_before_config_load()
}

/// Prepares resources that require a validated runtime configuration.
fn prepare_runtime_resources(config: &Config) -> TalosResult<RuntimeResourcesReport> {
    talos_upgrade::prepare_runtime_resources(config)
}
