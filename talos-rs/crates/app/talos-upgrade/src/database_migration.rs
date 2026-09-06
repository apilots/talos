use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use talos_error::{TalosError, TalosResult};

use crate::runner::UpgradeRunner;

/// Result of the database-owned SQL migration phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseMigrationReport {
    pub migration_dir: PathBuf,
    pub applied_versions: Vec<i64>,
    pub previously_applied_count: usize,
    pub total_applied_count: usize,
}

/// Run immutable SQL migrations through SQLx's independent migration ledger.
///
/// SQLx records applied scripts in `_sqlx_migrations` and skips them on later
/// startups. Product UpgradeTask state must not duplicate that data.
pub(crate) struct DatabaseMigrationRunner<'a> {
    pool: &'a SqlitePool,
    migration_dir: &'a Path,
}

impl<'a> DatabaseMigrationRunner<'a> {
    pub(crate) fn new(pool: &'a SqlitePool, migration_dir: &'a Path) -> Self {
        Self {
            pool,
            migration_dir,
        }
    }
}

impl UpgradeRunner for DatabaseMigrationRunner<'_> {
    type Report = DatabaseMigrationReport;

    async fn run(self) -> TalosResult<Self::Report> {
        let before = applied_versions(self.pool).await?;
        talos_store::migration::run_migrations(self.pool, self.migration_dir).await?;
        let after = applied_versions(self.pool).await?;
        let applied_versions = after
            .iter()
            .copied()
            .filter(|version| !before.contains(version))
            .collect();
        Ok(DatabaseMigrationReport {
            migration_dir: self.migration_dir.to_path_buf(),
            applied_versions,
            previously_applied_count: before.len(),
            total_applied_count: after.len(),
        })
    }
}

async fn applied_versions(pool: &SqlitePool) -> TalosResult<Vec<i64>> {
    let ledger_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| {
        TalosError::internal_with_source(
            "inspect SQLx migration ledger; technical details were withheld.",
            error,
        )
    })?;
    if ledger_exists == 0 {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(
        "SELECT version FROM _sqlx_migrations WHERE success = TRUE ORDER BY version ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| {
        TalosError::internal_with_source(
            "read SQLx migration ledger; technical details were withheld.",
            error,
        )
    })
}

#[cfg(test)]
#[path = "database_migration_tests.rs"]
mod tests;
