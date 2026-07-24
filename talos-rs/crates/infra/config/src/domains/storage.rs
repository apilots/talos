use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use talos_error::{TalosError, TalosResult};

use crate::TalosPaths;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseType {
    #[default]
    Sqlite,
    Postgres,
}

impl DatabaseType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }
}

impl fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Storage backend configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageBackendConfig {
    /// Backend type, currently only `sqlite` is supported.
    #[serde(default = "default_storage_database_type")]
    pub database_type: DatabaseType,

    /// Database connection string or file path.
    #[serde(default = "default_storage_connection_string")]
    pub connection_string: String,

    /// Connection pool size.
    #[serde(default = "default_storage_max_connections")]
    pub max_connections: u32,

    /// User file object storage configuration.
    #[serde(default)]
    pub files: FileStorageConfig,
}

/// Selects the configured OpenDAL backend used for file objects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileStorageBackendConfig {
    Fs(FsFileStorageConfig),
    S3(S3FileStorageConfig),
}

/// Configures one OpenDAL filesystem backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FsFileStorageConfig {
    pub root: PathBuf,
}

/// Configures one OpenDAL S3-compatible backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct S3FileStorageConfig {
    #[serde(default)]
    pub root: Option<String>,
    pub bucket: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub access_key_id: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
}

/// Hard limits applied to file metadata, uploads, and model turns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FileStorageLimits {
    pub max_upload_bytes: u64,
    pub max_files_per_turn: usize,
    pub max_turn_file_bytes: u64,
    pub max_filename_bytes: usize,
    pub max_tags: usize,
    pub max_tag_bytes: usize,
    pub max_metadata_entries: usize,
    pub max_metadata_key_bytes: usize,
    pub max_metadata_value_bytes: usize,
    pub max_metadata_bytes: usize,
}

impl Default for FileStorageLimits {
    fn default() -> Self {
        Self {
            max_upload_bytes: 50 * 1024 * 1024,
            max_files_per_turn: 5,
            max_turn_file_bytes: 20 * 1024 * 1024,
            max_filename_bytes: 255,
            max_tags: 20,
            max_tag_bytes: 64,
            max_metadata_entries: 32,
            max_metadata_key_bytes: 64,
            max_metadata_value_bytes: 2_048,
            max_metadata_bytes: 8_192,
        }
    }
}

/// Owns all object storage backends and file policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileStorageConfig {
    #[serde(default = "default_file_storage_id")]
    pub default: String,
    #[serde(default = "default_file_storage_backends")]
    pub backends: BTreeMap<String, FileStorageBackendConfig>,
    #[serde(default)]
    pub limits: FileStorageLimits,
    #[serde(default = "default_allowed_file_mime_types")]
    pub allowed_mime_types: BTreeSet<String>,
}

impl Default for FileStorageConfig {
    fn default() -> Self {
        Self {
            default: default_file_storage_id(),
            backends: default_file_storage_backends(),
            limits: FileStorageLimits::default(),
            allowed_mime_types: default_allowed_file_mime_types(),
        }
    }
}

impl FileStorageConfig {
    /// Validates backend identities, credentials, limits, and accepted media types.
    pub(crate) fn validate(&self) -> TalosResult<()> {
        if self.default.trim().is_empty() || self.default.trim() != self.default {
            return Err(TalosError::InvalidConfiguration(
                "storage.files.default must be a non-empty backend id without surrounding whitespace"
                    .to_string(),
            ));
        }
        if !self.backends.contains_key(&self.default) {
            return Err(TalosError::InvalidConfiguration(format!(
                "storage.files.default references unknown backend: {}",
                self.default
            )));
        }

        for (storage_id, backend) in &self.backends {
            validate_file_backend(storage_id, backend)?;
        }
        validate_file_limits(&self.limits)?;

        for mime_type in &self.allowed_mime_types {
            if mime_type.trim() != mime_type
                || !(infer::is_mime_supported(mime_type)
                    || matches!(
                        mime_type.as_str(),
                        "application/msword" | "application/vnd.ms-excel" | "text/csv"
                    ))
            {
                return Err(TalosError::InvalidConfiguration(format!(
                    "storage.files.allowed_mime_types contains unsupported MIME: {mime_type}"
                )));
            }
        }

        Ok(())
    }
}

fn validate_file_backend(storage_id: &str, backend: &FileStorageBackendConfig) -> TalosResult<()> {
    if storage_id.trim().is_empty() || storage_id.trim() != storage_id {
        return Err(TalosError::InvalidConfiguration(
            "storage.files.backends keys must be non-empty without surrounding whitespace"
                .to_string(),
        ));
    }
    match backend {
        FileStorageBackendConfig::Fs(file_system) => {
            let root = file_system.root.to_string_lossy();
            if root.trim().is_empty() || root.contains("${") {
                return Err(TalosError::InvalidConfiguration(format!(
                    "storage.files.backends.{storage_id}.root is empty or unresolved"
                )));
            }
        }
        FileStorageBackendConfig::S3(s3) => validate_s3_backend(storage_id, s3)?,
    }
    Ok(())
}

fn validate_s3_backend(storage_id: &str, s3: &S3FileStorageConfig) -> TalosResult<()> {
    if invalid_file_config_value(&s3.bucket) {
        return Err(TalosError::InvalidConfiguration(format!(
            "storage.files.backends.{storage_id}.bucket is empty or unresolved"
        )));
    }
    if let Some(root) = s3.root.as_deref() {
        if invalid_file_config_value(root) {
            return Err(TalosError::InvalidConfiguration(format!(
                "storage.files.backends.{storage_id}.root is empty or unresolved"
            )));
        }
        if root.split('/').any(|segment| matches!(segment, "." | "..")) {
            return Err(TalosError::InvalidConfiguration(format!(
                "storage.files.backends.{storage_id}.root must not contain `.` or `..` segments"
            )));
        }
    }
    for (field, value) in [
        ("region", s3.region.as_deref()),
        ("endpoint", s3.endpoint.as_deref()),
    ] {
        if value.is_some_and(invalid_file_config_value) {
            return Err(TalosError::InvalidConfiguration(format!(
                "storage.files.backends.{storage_id}.{field} is empty or unresolved"
            )));
        }
    }
    match (s3.access_key_id.as_deref(), s3.secret_access_key.as_deref()) {
        (None, None) => Ok(()),
        (Some(access_key_id), Some(secret_access_key))
            if !invalid_file_config_value(access_key_id)
                && !invalid_file_config_value(secret_access_key) =>
        {
            Ok(())
        }
        (Some(_), Some(_)) => Err(TalosError::InvalidConfiguration(format!(
            "storage.files.backends.{storage_id} credentials are empty or unresolved"
        ))),
        _ => Err(TalosError::InvalidConfiguration(format!(
            "storage.files.backends.{storage_id}.access_key_id and secret_access_key must be configured together"
        ))),
    }
}

fn validate_file_limits(limits: &FileStorageLimits) -> TalosResult<()> {
    for (name, value) in [
        ("max_upload_bytes", u128::from(limits.max_upload_bytes)),
        ("max_files_per_turn", limits.max_files_per_turn as u128),
        (
            "max_turn_file_bytes",
            u128::from(limits.max_turn_file_bytes),
        ),
        ("max_filename_bytes", limits.max_filename_bytes as u128),
        ("max_tags", limits.max_tags as u128),
        ("max_tag_bytes", limits.max_tag_bytes as u128),
        ("max_metadata_entries", limits.max_metadata_entries as u128),
        (
            "max_metadata_key_bytes",
            limits.max_metadata_key_bytes as u128,
        ),
        (
            "max_metadata_value_bytes",
            limits.max_metadata_value_bytes as u128,
        ),
        ("max_metadata_bytes", limits.max_metadata_bytes as u128),
    ] {
        if value == 0 {
            return Err(TalosError::InvalidConfiguration(format!(
                "storage.files.limits.{name} must be > 0"
            )));
        }
    }
    if limits.max_metadata_value_bytes > limits.max_metadata_bytes {
        return Err(TalosError::InvalidConfiguration(
            "storage.files.limits.max_metadata_value_bytes must be <= max_metadata_bytes"
                .to_string(),
        ));
    }
    Ok(())
}

fn invalid_file_config_value(value: &str) -> bool {
    value.trim().is_empty() || value.contains("${")
}

fn default_file_storage_id() -> String {
    "local".to_string()
}

fn default_file_storage_backends() -> BTreeMap<String, FileStorageBackendConfig> {
    BTreeMap::from([(
        default_file_storage_id(),
        FileStorageBackendConfig::Fs(FsFileStorageConfig {
            root: PathBuf::from("files"),
        }),
    )])
}

fn default_allowed_file_mime_types() -> BTreeSet<String> {
    [
        "application/msword",
        "application/pdf",
        "application/vnd.ms-excel",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "image/jpeg",
        "image/png",
        "image/webp",
        "text/csv",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_storage_database_type() -> DatabaseType {
    DatabaseType::Sqlite
}

fn default_storage_connection_string() -> String {
    TalosPaths::resolve()
        .sqlite_db
        .to_string_lossy()
        .to_string()
}

fn default_storage_max_connections() -> u32 {
    5
}

impl Default for StorageBackendConfig {
    fn default() -> Self {
        Self {
            database_type: default_storage_database_type(),
            connection_string: default_storage_connection_string(),
            max_connections: default_storage_max_connections(),
            files: FileStorageConfig::default(),
        }
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
