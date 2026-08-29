use talos_config::{
    Config, ConfigTemplateRefreshReport, ConfigUpgradeReport, DatabaseMigrationsInstallReport,
};
use talos_error::TalosResult;

const TALOS_CONFIG_ENV: &str = "TALOS_CONFIG";

/// Resource preparation completed before strict configuration loading.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BeforeConfigLoadReport {
    pub default_config_template: Option<ConfigTemplateRefreshReport>,
    pub default_config: Option<ConfigUpgradeReport>,
    pub skipped_explicit_config: bool,
}

/// Resource preparation completed after strict configuration loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeResourcesReport {
    pub database_migrations: DatabaseMigrationsInstallReport,
}

pub(crate) fn prepare_before_config_load() -> TalosResult<BeforeConfigLoadReport> {
    prepare_before_config_load_with_explicit_config(std::env::var_os(TALOS_CONFIG_ENV).is_some())
}

pub(crate) fn prepare_runtime_resources(config: &Config) -> TalosResult<RuntimeResourcesReport> {
    tracing::info!("runtime resource preparation started");
    let database_migrations = talos_config::install_database_migrations(config)?;
    tracing::info!(
        source = %database_migrations.source_dir.display(),
        target = %database_migrations.target_dir.display(),
        backend = %database_migrations.backend_dir.display(),
        database_type = %database_migrations.database_type,
        changed = database_migrations.changed,
        "database migration resources prepared"
    );
    let report = RuntimeResourcesReport {
        database_migrations,
    };
    tracing::info!(report = ?report, "runtime resource preparation completed");
    Ok(report)
}

fn prepare_before_config_load_with_explicit_config(
    explicit_config: bool,
) -> TalosResult<BeforeConfigLoadReport> {
    tracing::info!("pre-config resource preparation started");
    if explicit_config {
        tracing::info!(
            env = TALOS_CONFIG_ENV,
            "default config preparation skipped because explicit config is set"
        );
        let report = BeforeConfigLoadReport {
            default_config_template: None,
            default_config: None,
            skipped_explicit_config: true,
        };
        tracing::info!(report = ?report, "pre-config resource preparation completed");
        return Ok(report);
    }

    let default_config_template = talos_config::refresh_default_config_template()?;
    tracing::info!(
        source = %default_config_template.source_path.display(),
        target = %default_config_template.target_path.display(),
        changed = default_config_template.changed,
        "default config template refreshed"
    );

    let default_config = talos_config::upgrade_default_config()?;
    if default_config.changed {
        tracing::info!(
            path = %default_config.source_path.display(),
            backup = ?default_config.backup_path,
            added = ?default_config.added_paths,
            removed = ?default_config.removed_paths,
            defaulted = ?default_config.defaulted_paths,
            "default config upgraded"
        );
    } else {
        tracing::info!(
            path = %default_config.source_path.display(),
            "default config already current"
        );
    }

    let report = BeforeConfigLoadReport {
        default_config_template: Some(default_config_template),
        default_config: Some(default_config),
        skipped_explicit_config: false,
    };
    tracing::info!(report = ?report, "pre-config resource preparation completed");
    Ok(report)
}

#[cfg(test)]
#[path = "preparation_tests.rs"]
mod tests;
