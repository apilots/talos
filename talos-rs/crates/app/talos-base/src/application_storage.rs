use std::sync::Arc;

use talos_config::Config;
use talos_error::TalosResult;
use talos_storage::{create_storage, SqliteStorage, StorageConfig};
use talos_store::migration::run_migrations;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};

/// Builds the SQLite and Profile stores required by application composition.
pub(crate) async fn build_application_storage(
    config: &Config,
) -> TalosResult<(Arc<SqliteStorage>, Arc<dyn ProfileStore>)> {
    let storage = create_storage(&StorageConfig {
        database_type: config.storage.database_type,
        connection_string: config.storage.connection_string.clone(),
        max_connections: config.storage.max_connections,
    })
    .await?;
    run_migrations(storage.pool()).await?;
    let storage = Arc::new(storage);
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    Ok((storage, profile_store))
}
