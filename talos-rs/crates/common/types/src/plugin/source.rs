use super::MarketplaceSourceId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct MarketplaceSource {
    pub id: MarketplaceSourceId,
    pub source_type: MarketplaceSourceType,
    pub kind: MarketplaceSourceKind,
    pub display_name: String,
    pub location: MarketplaceSourceLocation,
    pub sparse_paths: Vec<String>,
    pub last_checked_commit: Option<String>,
    pub last_checked_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, TS,
)]
#[serde(rename_all = "snake_case")]
pub enum MarketplaceSourceType {
    Official,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum MarketplaceSourceKind {
    Git,
    LocalFolder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketplaceSourceLocation {
    Git { url: String, reference: String },
    LocalFolder { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketplaceSourceInput {
    Git {
        url: String,
        reference: String,
        sparse_paths: Vec<String>,
    },
    LocalFolder {
        path: PathBuf,
        sparse_paths: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginRootConfig {
    pub kind: PluginRootKind,
    pub path: PathBuf,
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginRootKind {
    Skill,
    McpServer,
    Package,
    Feature,
}
