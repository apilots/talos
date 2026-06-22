use super::{MarketplaceSourceId, PluginId, PluginInstallId, PluginKind};
use crate::McpServerDefinition;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginInstall {
    pub id: PluginInstallId,
    pub plugin_id: PluginId,
    pub kind: PluginKind,
    pub name: String,
    pub installed_path: PathBuf,
    #[serde(default)]
    pub exposed_skill_root: Option<PathBuf>,
    #[serde(default)]
    pub exposed_mcp_servers: Vec<McpServerDefinition>,
    pub source: PluginInstallSource,
    #[ts(type = "number")]
    pub installed_at_ms: i64,
    #[ts(type = "number")]
    pub updated_at_ms: i64,
    pub file_manifest: FileManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginInstallSource {
    Marketplace {
        source_id: MarketplaceSourceId,
        url: String,
        reference: String,
        commit: String,
        path_in_source: String,
    },
    LocalPath {
        source_id: MarketplaceSourceId,
        path: PathBuf,
        path_in_source: String,
    },
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS, Default)]
pub struct FileManifest {
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct FileEntry {
    pub relative_path: String,
    pub sha256: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
}
