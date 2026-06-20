use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpServerDefinition {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub transport: McpTransportDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpServerRuntimeDescriptor {
    pub server: McpServerDefinition,
    pub source: McpServerSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpServerSource {
    Config,
    Plugin {
        plugin_id: super::PluginId,
        install_id: super::PluginInstallId,
        installed_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct McpServerSnapshot {
    pub server: McpServerDefinition,
    pub source: McpServerSource,
    pub status: McpServerStatus,
    #[serde(default)]
    pub tools: Vec<McpToolDescriptor>,
    #[serde(default)]
    pub resources: Vec<McpResourceDescriptor>,
    #[serde(default)]
    pub resource_templates: Vec<McpResourceTemplateDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum McpServerStatus {
    NotStarted,
    Starting,
    Ready,
    Failed { message: String },
    Disabled,
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
    Text {
        text: String,
    },
    Image {
        data: String,
        mime_type: String,
    },
    Resource {
        resource: McpResourceContents,
    },
    Unknown {
        #[ts(type = "unknown")]
        raw: Value,
    },
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
    Unknown {
        #[ts(type = "unknown")]
        raw: Value,
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
