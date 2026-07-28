use std::path::Path;

use sqlx::SqlitePool;
use talos_error::{TalosError, TalosResult};

/// Applies the complete immutable Store schema after Storage connects.
///
/// Application composition must call this before constructing database stores.
/// Storage owns connections only; all production DDL belongs to these migrations.
pub async fn run_migrations(pool: &SqlitePool, migration_dir: &Path) -> TalosResult<()> {
    let migrator = sqlx::migrate::Migrator::new(migration_dir)
        .await
        .map_err(|error| {
            TalosError::StorageError(format!(
                "load store migrations from {}: {error}",
                migration_dir.display()
            ))
        })?;
    migrator
        .run(pool)
        .await
        .map_err(|error| TalosError::StorageError(format!("apply store migrations: {error}")))
}

#[cfg(test)]
pub(crate) fn test_migrations_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite")
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
