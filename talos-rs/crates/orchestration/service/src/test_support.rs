use std::path::Path;

use sqlx::SqlitePool;

pub(crate) async fn run_test_migrations(pool: &SqlitePool) {
    let migration_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    talos_store::migration::run_migrations(pool, &migration_dir)
        .await
        .expect("test migrations should run");
}
