use std::collections::BTreeMap;
use std::sync::Arc;

use opendal::services::{Fs, S3};
use opendal::Operator;
use talos_config::{Config, FileStorageBackendConfig, FileStorageConfig};
use talos_error::{TalosError, TalosResult};
use talos_storage::{create_storage, SqliteStorage, StorageConfig};
use talos_store::file::{DatabaseFileStore, FileStore};
use talos_store::profile::{DatabaseProfileStore, ProfileStore};

/// Connect the SQLite storage used by startup upgrades and application stores.
pub(crate) async fn connect_application_storage(
    config: &Config,
) -> TalosResult<Arc<SqliteStorage>> {
    create_storage(&StorageConfig {
        database_type: config.storage.database_type,
        connection_string: config.storage.connection_string.clone(),
        max_connections: config.storage.max_connections,
    })
    .await
    .map(Arc::new)
}

/// Build database stores only after every startup upgrade has succeeded.
pub(crate) fn build_database_stores(
    storage: &Arc<SqliteStorage>,
) -> (
    Arc<SqliteStorage>,
    Arc<dyn ProfileStore>,
    Arc<dyn FileStore>,
) {
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let file_store: Arc<dyn FileStore> = Arc::new(DatabaseFileStore::new(storage.pool().clone()));
    (storage.clone(), profile_store, file_store)
}

/// Builds the configured OpenDAL operators keyed by persistent storage ID.
pub(crate) fn build_file_operators(
    config: &FileStorageConfig,
) -> TalosResult<BTreeMap<String, Operator>> {
    config
        .backends
        .iter()
        .map(|(storage_id, backend)| {
            let operator = match backend {
                FileStorageBackendConfig::Fs(file_system) => {
                    let root = file_system.root.to_str().ok_or_else(|| {
                        TalosError::new(
                            talos_error::ErrorCode::InvalidRequest,
                            "storage.files filesystem root is not valid UTF-8".to_string(),
                        )
                    })?;
                    Operator::new(Fs::default().root(root))
                        .map_err(|error| operator_error("build filesystem operator", &error))?
                        .finish()
                }
                FileStorageBackendConfig::S3(s3) => {
                    let mut builder = S3::default()
                        .root(s3.root.as_deref().unwrap_or("/"))
                        .bucket(&s3.bucket);
                    if let Some(region) = s3.region.as_deref() {
                        builder = builder.region(region);
                    }
                    if let Some(endpoint) = s3.endpoint.as_deref() {
                        builder = builder.endpoint(endpoint);
                    }
                    if let (Some(access_key_id), Some(secret_access_key)) =
                        (s3.access_key_id.as_deref(), s3.secret_access_key.as_deref())
                    {
                        builder = builder
                            .access_key_id(access_key_id)
                            .secret_access_key(secret_access_key);
                    }
                    Operator::new(builder)
                        .map_err(|error| operator_error("build S3 operator", &error))?
                        .finish()
                }
            };
            Ok((storage_id.clone(), operator))
        })
        .collect()
}

fn operator_error(context: &str, error: &opendal::Error) -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::InvalidRequest,
        format!("{context} failed: {:?}", error.kind()),
    )
}

#[cfg(test)]
#[path = "application_storage_tests.rs"]
mod tests;
