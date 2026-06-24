use super::{MarketplaceSourceId, PluginId, PluginInstallId, PluginKind};
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
    pub enabled: bool,
    pub source: PluginInstallSource,
    pub content_digest: ContentDigest,
    #[ts(type = "number")]
    pub installed_at_ms: i64,
    #[ts(type = "number")]
    pub updated_at_ms: i64,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ContentDigest {
    pub algorithm: ContentDigestAlgorithm,
    pub value: String,
    #[ts(type = "number")]
    pub file_count: u64,
    #[ts(type = "number")]
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContentDigestAlgorithm {
    Sha256,
}
