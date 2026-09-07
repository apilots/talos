use std::path::{Path, PathBuf};
use std::time::Instant;

use sqlx::SqlitePool;
use talos_config::Config;
use talos_error::{ErrorCode, TalosError, TalosResult};

use runner::UpgradeRunner;

mod database_migration;
mod dream_memory;
mod preparation;
mod runner;
mod workspace_layout;

pub use database_migration::DatabaseMigrationReport;
pub use preparation::{BeforeConfigLoadReport, RuntimeResourcesReport};
pub use runner::UpgradeRunReport;

/// Inputs for the serial upgrade boundary executed before runtime assembly.
pub struct StartupUpgradeParams<'a> {
    pub config: &'a Config,
    pub database_pool: &'a SqlitePool,
    pub database_migration_dir: &'a Path,
}

/// Complete result of all startup migrations and product upgrade tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupUpgradeReport {
    pub database: DatabaseMigrationReport,
    pub product: UpgradeRunReport,
    pub workspace_prefix: PathBuf,
}

/// Run all startup migrations serially before any Service or Agent is built.
///
/// Database SQL migrations keep their SQLx ledger. Non-database product tasks
/// use generic UpgradeTask state. Success authorizes runtime assembly.
pub async fn run_startup_upgrades(
    params: StartupUpgradeParams<'_>,
) -> TalosResult<StartupUpgradeReport> {
    run_startup_upgrades_for_version(params, env!("CARGO_PKG_VERSION")).await
}

async fn run_startup_upgrades_for_version(
    params: StartupUpgradeParams<'_>,
    current_version: &str,
) -> TalosResult<StartupUpgradeReport> {
    let started_at = Instant::now();
    tracing::info!(
        migration_dir = %params.database_migration_dir.display(),
        "startup upgrade sequence started"
    );
    let database_started_at = Instant::now();
    let database = database_migration::DatabaseMigrationRunner::new(
        params.database_pool,
        params.database_migration_dir,
    )
    .run()
    .await
    .map_err(|error| {
        tracing::error!(
            migration_dir = %params.database_migration_dir.display(),
            elapsed_ms = database_started_at.elapsed().as_millis(),
            error = %error,
            "database migration phase failed"
        );
        error
    })?;
    tracing::info!(
        report = ?database,
        elapsed_ms = database_started_at.elapsed().as_millis(),
        "database migration phase completed"
    );
    if database.applied_versions.is_empty() {
        tracing::info!(
            total_applied = database.total_applied_count,
            "database migrations already current"
        );
    } else {
        tracing::info!(
            applied_versions = ?database.applied_versions,
            total_applied = database.total_applied_count,
            "database migrations applied"
        );
    }
    let tasks = [
        dream_memory::upgrade_task(),
        workspace_layout::upgrade_task(),
    ];
    let product = runner::ProductUpgradeRunner::for_version(
        params.config,
        &tasks,
        workspace_layout::legacy_installation_exists,
        current_version,
    )?
    .run()
    .await?;
    tracing::info!(report = ?product, "product upgrade task phase completed");
    let workspace_task_deferred = product
        .deferred
        .iter()
        .any(|task_id| task_id == workspace_layout::WORKSPACE_LAYOUT_V1_UPGRADE_ID);
    if workspace_task_deferred {
        let paths = params.config.talos_paths();
        let context = runner::UpgradeContext {
            config: params.config,
            paths: &paths,
        };
        if workspace_layout::legacy_installation_exists(&context)? {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                format!(
                    "workspace layout upgrade requires Talos 0.1.8; current version is {current_version}"
                ),
            ));
        }
    }
    let workspace_prefix =
        workspace_layout::initialize_workspace_layout(params.config).map_err(|error| {
            tracing::error!(error = %error, "workspace layout initialization failed");
            error
        })?;
    tracing::info!(
        workspace_prefix = %workspace_prefix.display(),
        "workspace layout initialized"
    );
    let report = StartupUpgradeReport {
        database,
        product,
        workspace_prefix,
    };
    tracing::info!(
        report = ?report,
        elapsed_ms = started_at.elapsed().as_millis(),
        "startup upgrade sequence completed"
    );
    Ok(report)
}

/// Prepare product-managed resources needed before strict config loading.
pub fn prepare_before_config_load() -> TalosResult<BeforeConfigLoadReport> {
    preparation::prepare_before_config_load()
}

/// Prepare database migration resources after strict config loading.
pub fn prepare_runtime_resources(config: &Config) -> TalosResult<RuntimeResourcesReport> {
    preparation::prepare_runtime_resources(config)
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
