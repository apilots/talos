use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
pub use talos_config::{McpAuthentication, McpCredentialBinding, McpOAuthConfig};
use ts_rs::TS;

/// Stable identity shared by configuration, plugin manifests and management cursors.
/// Names are ASCII identifiers, at most 256 bytes, and contain no path separators.
pub fn validate_mcp_server_name(name: &str) -> talos_error::TalosResult<()> {
    if name.is_empty()
        || name.len() > 256
        || name.contains("__")
        || !name
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-')
    {
        return Err(talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidName,
            "MCP names must contain 1–256 ASCII letters, digits, underscores or hyphens; double underscores are reserved for tool routing.",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct McpServerDefinition {
    #[serde(default)]
    #[ts(optional)]
    pub credentials: Option<McpCredentialBinding>,
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub transport: McpTransportDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum McpTransportDefinition {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    StreamableHttp {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum McpTransportKind {
    Stdio,
    StreamableHttp,
}

impl McpTransportDefinition {
    pub fn kind(&self) -> McpTransportKind {
        match self {
            Self::Stdio { .. } => McpTransportKind::Stdio,
            Self::StreamableHttp { .. } => McpTransportKind::StreamableHttp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpToolDescriptor {
    pub server_name: String,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    #[ts(type = "unknown")]
    pub input_schema: Value,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub output_schema: Option<Value>,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub annotations: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpResourceDescriptor {
    pub server_name: String,
    pub uri: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpResourceTemplateDescriptor {
    pub server_name: String,
    pub uri_template: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// One bounded resource page; the cursor continues the same server query.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpResourceListResponse {
    pub data: Vec<McpResourceDescriptor>,
    pub next_cursor: Option<String>,
}

/// One bounded resource-template page.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpResourceTemplateListResponse {
    pub data: Vec<McpResourceTemplateDescriptor>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpServerRuntimeDescriptor {
    pub content_revision: Option<super::ContentDigest>,
    pub server: McpServerDefinition,
    pub source: McpServerSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum McpServerSource {
    Config,
    Plugin {
        plugin_id: super::PluginId,
        install_id: super::PluginInstallId,
    },
}

/// A safe connection summary. Commands, environment, headers and URLs are never returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInfo {
    pub name: String,
    pub display_name: Option<String>,
    pub source: McpServerSource,
    pub transport: McpTransportKind,
    pub status: McpServerStatus,
    pub tool_count: usize,
}

/// Omitted/null cursor starts from the beginning; omitted/null limit defaults to 50.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpServerListRequest {
    #[serde(rename = "nodeId")]
    #[ts(optional)]
    pub node_id: Option<String>,
    #[ts(optional)]
    pub cursor: Option<String>,
    #[ts(optional)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpServerListResponse {
    pub data: Vec<McpServerInfo>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum McpServerStatus {
    NotStarted,
    Starting,
    Ready,
    Failed { error: talos_error::ApiError },
    Exited { error: talos_error::ApiError },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpCallToolRequest {
    pub server_name: String,
    pub tool_name: String,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpCallToolResponse {
    #[serde(default)]
    pub content: Vec<McpContent>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    #[ts(type = "unknown | null")]
    pub structured_content: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: McpResourceContents },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpResourceContents {
    Text {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
    Blob {
        uri: String,
        mime_type: Option<String>,
        blob: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpReadResourceRequest {
    pub server_name: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpReadResourceResponse {
    #[serde(default)]
    pub contents: Vec<McpResourceContents>,
}
