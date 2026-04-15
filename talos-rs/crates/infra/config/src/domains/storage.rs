use serde::{Deserialize, Serialize};
use std::fmt;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        }
    }
}
