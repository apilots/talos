use talos_config::{Config, TalosPaths};
use talos_error::TalosResult;
use talos_logging::LogGuard;
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
    let startup: TalosResult<_> = (|| {
        let mut settings = talos_config::load_startup_logging(&paths)?;
        settings.logging.stdout = stdout;
        let guard = talos_logging::init_with_config(settings.logging, &settings.timezone)?;
        Ok((guard, settings.warning, settings.source_path))
    })();
    let (log_guard, warning, source_path) = startup.inspect_err(|error| {
        // No logging sink exists yet; stderr is the startup diagnostic boundary.
        eprintln!("startup logging failed: {error:#?}");
    })?;
    if let Some(warning) = warning {
        tracing::warn!("{warning}");
    }
    info!(config_file = %source_path.display(), log_file = %paths.log_file.display(), "startup logging initialized");
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
