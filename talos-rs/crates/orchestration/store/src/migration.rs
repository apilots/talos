use sqlx::SqlitePool;
use talos_error::{TalosError, TalosResult};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Applies immutable store-owned schema migrations after storage connects.
///
/// Application composition calls this once before constructing database stores.
/// The runner owns only versioned Store schema changes; it does not replace the
/// existing Storage schema bootstrap or perform data repair for local test files.
pub async fn run_migrations(pool: &SqlitePool) -> TalosResult<()> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|error| TalosError::StorageError(format!("apply store migrations: {error}")))
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
