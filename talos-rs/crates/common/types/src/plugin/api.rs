use super::{
    MarketplaceSource, MarketplaceSourceId, MarketplaceSourceInput, Plugin,
    PluginDocumentMediaType, PluginDocumentRole, PluginId, PluginInstall, PluginInstallId,
    PluginKind, PluginWarning,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS, Default)]
pub struct PluginSourceListRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceListResponse {
    pub sources: Vec<MarketplaceSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceAddRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub source: MarketplaceSourceInput,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceAddResponse {
    pub source: MarketplaceSource,
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<PluginWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceRefreshRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub source_id: MarketplaceSourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceRefreshResponse {
    pub source: MarketplaceSource,
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<PluginWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceRemoveRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub source_id: MarketplaceSourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceRemoveResponse {
    pub source_id: MarketplaceSourceId,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginListRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub kind: Option<PluginKind>,
    pub status: Option<PluginListStatusFilter>,
    pub source_id: Option<MarketplaceSourceId>,
    pub include_disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginListStatusFilter {
    Installed,
    Disabled,
    UpdateAvailable,
    SourceMissing,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginListResponse {
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<PluginWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginReadRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub plugin_id: PluginId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginReadResponse {
    pub plugin: Plugin,
    pub documents: Vec<PluginDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDocument {
    pub role: PluginDocumentRole,
    pub path: String,
    pub title: String,
    pub media_type: PluginDocumentMediaType,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSearchRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub query: Option<String>,
    pub source_id: Option<MarketplaceSourceId>,
    pub kind: Option<PluginKind>,
    pub status: Option<PluginSearchStatusFilter>,
    pub refresh: PluginSearchRefreshPolicy,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginSearchStatusFilter {
    Any,
    NotInstalled,
    Installed,
    UpdateAvailable,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginSearchRefreshPolicy {
    CacheOnly,
    RefreshStale,
    ForceRefresh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSearchResponse {
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<PluginWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginInstallRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub target: PluginInstallTarget,
    pub enable: Option<bool>,
    pub conflict_policy: PluginInstallConflictPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginInstallTarget {
    ManualMcpServer {
        server: Box<crate::McpServerDefinition>,
        #[ts(optional)]
        credentials: Option<crate::McpCredentialValues>,
    },
    Plugin {
        plugin_id: PluginId,
        expected_source_commit: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginInstallConflictPolicy {
    Reject,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginInstallResponse {
    pub plugin: Plugin,
    pub install: PluginInstall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginUpdateRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub install_id: PluginInstallId,
    pub expected_source_commit: Option<String>,
    pub conflict_policy: PluginUpdateConflictPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginUpdateConflictPolicy {
    RejectLocalChanges,
    ReplaceLocalChanges,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginUpdateResponse {
    pub plugin: Plugin,
    pub install: PluginInstall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginUninstallRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub install_id: PluginInstallId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginUninstallResponse {
    pub install_id: PluginInstallId,
    pub uninstalled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginEnableRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub install_id: PluginInstallId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginEnableResponse {
    pub plugin: Plugin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDisableRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub install_id: PluginInstallId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDisableResponse {
    pub plugin: Plugin,
}
