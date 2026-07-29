use talos_config::{Config, TalosPaths};
use talos_error::TalosResult;
use talos_logging::{LogGuard, StartupLoggingOptions};
use talos_upgrade::{AfterUpgradeReport, BeforeUpgradeReport};
use tracing::{error, info};

use super::TalosServerOptions;

/// Initializes logging and runs pre-configuration startup upgrades.
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
    let report = upgrade_before()?;
    info!(report = ?report, "upgrade_before completed");
    Ok(log_guard)
}

/// Runs upgrade steps that must happen before runtime configuration is loaded.
pub fn upgrade_before() -> TalosResult<BeforeUpgradeReport> {
    talos_upgrade::upgrade_before().map_err(|error| {
        error!("upgrade_before failed: {}", error);
        error
    })
}

/// Runs upgrade steps that require a validated runtime configuration.
pub fn upgrade_after(config: &Config) -> TalosResult<AfterUpgradeReport> {
    talos_upgrade::upgrade_after(config).map_err(|error| {
        error!("upgrade_after failed: {}", error);
        error
    })
}
