use std::path::Path;

use sqlx::migrate::MigrateError;
use sqlx::{Executor, SqlitePool};
use talos_error::{TalosError, TalosResult};

/// Applies the complete immutable Store schema after Storage connects.
///
/// Application composition must call this before constructing database stores.
/// Storage owns connections only; all production DDL belongs to these migrations.
pub async fn run_migrations(pool: &SqlitePool, migration_dir: &Path) -> TalosResult<()> {
    let migrator = sqlx::migrate::Migrator::new(migration_dir)
        .await
        .map_err(|error| {
            let diagnostic = format!(
                "load store migrations from {}: {error}",
                migration_dir.display()
            );
            TalosError::internal_with_source(
                "Load store migrations; technical details were withheld.",
                error,
            )
            .context(diagnostic)
        })?;
    let mut result = migrator.run(pool).await;
    let blocked_by_legacy_trigger = matches!(
        &result,
        Err(MigrateError::Execute(sqlx::Error::Database(error)))
            if error.message() == "error in trigger sessions_cleanup_after_delete: no such table: main.legacy_timeline_events"
    );
    let repair = migrator
        .iter()
        .find(|migration| migration.version == 202611190002);
    if let (true, Some(repair)) = (blocked_by_legacy_trigger, repair) {
        // The previously applied history upgrade removed the legacy table.
        // Apply the idempotent trigger repair before retrying the blocked rename.
        // The retry records the same repair normally in the SQLx ledger.
        let rename_pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version = 202611180004 AND success)
                AND NOT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version >= 202611180005)",
        )
        .fetch_one(pool)
        .await
        .map_err(apply_migration_error)?;
        if rename_pending {
            let mut transaction = pool.begin().await.map_err(apply_migration_error)?;
            let repair_sql: &str = &repair.sql;
            (&mut *transaction)
                .execute(repair_sql)
                .await
                .map_err(apply_migration_error)?;
            transaction.commit().await.map_err(apply_migration_error)?;
            result = migrator.run(pool).await;
        }
    }
    result.map_err(apply_migration_error)
}

fn apply_migration_error(error: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::internal_with_source(
        "apply store migrations; technical details were withheld.",
        error,
    )
}

#[cfg(test)]
pub(crate) fn test_migrations_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite")
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "error_migration_tests.rs"]
mod error_tests;

#[cfg(test)]
#[path = "event_generation_tests.rs"]
mod event_generation_tests;
