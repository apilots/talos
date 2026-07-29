use talos_config::{
    Config, ConfigTemplateRefreshReport, ConfigUpgradeReport, DatabaseMigrationsInstallReport,
};
use talos_error::TalosResult;

const TALOS_CONFIG_ENV: &str = "TALOS_CONFIG";

/// Upgrade work completed before strict config loading.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BeforeUpgradeReport {
    pub default_config_template: Option<ConfigTemplateRefreshReport>,
    pub default_config: Option<ConfigUpgradeReport>,
    pub skipped_explicit_config: bool,
}

/// Upgrade work completed after strict config loading and runtime validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfterUpgradeReport {
    pub database_migrations: DatabaseMigrationsInstallReport,
}

/// Run product-managed upgrades that must happen before strict config loading.
pub fn upgrade_before() -> TalosResult<BeforeUpgradeReport> {
    upgrade_before_with_explicit_config(std::env::var_os(TALOS_CONFIG_ENV).is_some())
}

/// Run product-managed upgrades that require a validated config object.
pub fn upgrade_after(config: &Config) -> TalosResult<AfterUpgradeReport> {
    tracing::info!("upgrade_after started");
    let database_migrations = talos_config::install_database_migrations(config)?;
    tracing::info!(
        source = %database_migrations.source_dir.display(),
        target = %database_migrations.target_dir.display(),
        backend = %database_migrations.backend_dir.display(),
        database_type = %database_migrations.database_type,
        changed = database_migrations.changed,
        "database migration resources installed"
    );
    tracing::info!(
        report = ?database_migrations,
        "upgrade_after completed"
    );
    Ok(AfterUpgradeReport {
        database_migrations,
    })
}

fn upgrade_before_with_explicit_config(explicit_config: bool) -> TalosResult<BeforeUpgradeReport> {
    tracing::info!("upgrade_before started");
    if explicit_config {
        tracing::info!(
            env = TALOS_CONFIG_ENV,
            "default config upgrade skipped because explicit config is set"
        );
        let report = BeforeUpgradeReport {
            default_config_template: None,
            default_config: None,
            skipped_explicit_config: true,
        };
        tracing::info!(report = ?report, "upgrade_before completed");
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

    let report = BeforeUpgradeReport {
        default_config_template: Some(default_config_template),
        default_config: Some(default_config),
        skipped_explicit_config: false,
    };
    tracing::info!(report = ?report, "upgrade_before completed");
    Ok(report)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
