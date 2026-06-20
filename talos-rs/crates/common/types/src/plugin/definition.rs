use super::{MarketplaceSourceId, PluginId, PluginInstallId};
use crate::McpServerDefinition;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub short_description: Option<String>,
    pub metadata: Option<SkillManifestMetadata>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillManifestMetadata {
    pub display_name: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginMetadata {
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub short_description: Option<String>,
    pub icon: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginDocumentRef {
    pub role: PluginDocumentRole,
    pub path: String,
    pub title: String,
    pub media_type: PluginDocumentMediaType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginDocumentRole {
    Primary,
    Readme,
    Manifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginDocumentMediaType {
    Markdown,
    Json,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct Plugin {
    pub id: PluginId,
    pub kind: PluginKind,
    pub metadata: PluginMetadata,
    #[serde(default)]
    pub documents: Vec<PluginDocumentRef>,
    pub source_id: Option<MarketplaceSourceId>,
    pub path_in_source: Option<String>,
    pub source_commit: Option<String>,
    pub status: PluginStatus,
    pub definition: PluginDefinition,
    pub warnings: Vec<PluginWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Skill,
    McpServer,
    Package,
    Feature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PluginStatus {
    NotInstalled,
    Installed {
        install_id: PluginInstallId,
        installed_path: PathBuf,
        installed_commit: Option<String>,
        enabled: bool,
    },
    UpdateAvailable {
        install_id: PluginInstallId,
        installed_path: PathBuf,
        installed_commit: Option<String>,
        latest_commit: String,
        enabled: bool,
    },
    SourceMissing {
        install_id: PluginInstallId,
        installed_path: PathBuf,
        installed_commit: Option<String>,
        enabled: bool,
    },
    Conflict {
        reason: PluginConflictReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", content = "plugin", rename_all = "snake_case")]
pub enum PluginDefinition {
    Skill(SkillPlugin),
    McpServer(McpServerPlugin),
    Package(PackagePlugin),
    Feature(FeaturePlugin),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillPlugin {
    pub skill_md_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpServerPlugin {
    pub server: McpServerDefinition,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum McpServerTransportKind {
    Stdio,
    StreamableHttp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackagePlugin {
    pub name: String,
    pub package_kind: PackageKind,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    Npm,
    Python,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct FeaturePlugin {
    pub name: String,
    pub feature_kind: FeatureKind,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    Ui,
    Runtime,
    Integration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PluginWarning {
    pub code: PluginWarningCode,
    pub message: String,
    pub kind: Option<PluginKind>,
    pub source_id: Option<MarketplaceSourceId>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginWarningCode {
    InvalidManifest,
    MissingRequiredField,
    UnsupportedPluginKind,
    NestedPlugin,
    PathEscapedSource,
    InvalidSourceLayout,
    OrphanInstallDirectory,
    SourceRefreshFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum PluginConflictReason {
    SameNameDifferentSource,
    TargetPathExists,
    TargetPathMissing,
    LocalChangesDetected,
    UnsupportedKind,
}
