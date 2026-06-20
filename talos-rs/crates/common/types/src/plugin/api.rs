use super::{
    MarketplaceSource, MarketplaceSourceId, MarketplaceSourceInput, Plugin, PluginId,
    PluginInstall, PluginInstallId, PluginKind, PluginWarning,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS, Default)]
pub struct PluginSourceListRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceListResponse {
    pub sources: Vec<MarketplaceSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceAddRequest {
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
    pub source_id: MarketplaceSourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSourceRemoveResponse {
    pub source_id: MarketplaceSourceId,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginListRequest {
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
    pub plugin_id: PluginId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginReadResponse {
    pub plugin: Plugin,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginSearchRequest {
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
    pub target: PluginInstallTarget,
    pub enable: Option<bool>,
    pub conflict_policy: PluginInstallConflictPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginInstallTarget {
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
    pub install_id: PluginInstallId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginUninstallResponse {
    pub install_id: PluginInstallId,
    pub uninstalled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginEnableRequest {
    pub install_id: PluginInstallId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginEnableResponse {
    pub plugin: Plugin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDisableRequest {
    pub install_id: PluginInstallId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDisableResponse {
    pub plugin: Plugin,
}
