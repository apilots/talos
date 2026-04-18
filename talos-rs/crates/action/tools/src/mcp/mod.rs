mod client;
mod protocol;
pub mod transport;

pub use client::McpClient;
pub use protocol::McpToolDefinition;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::ToolDefinition;
use tokio::sync::RwLock;

use crate::mcp::transport::{HttpTransport, McpTransport, StdioTransport};
use crate::output::ToolOutputPayload;
use crate::spec::{
    default_governance_for_source, RegisteredToolSpec, ToolParallelism, ToolPayloadKind, ToolSource,
};
use crate::state::sha256_hex;
use crate::ToolContext;

/// Prefix used in tool names for MCP tools: `mcp__<server>__<tool>`.
pub const MCP_TOOL_NAME_PREFIX: &str = "mcp__";
const MCP_DELIMITER: &str = "__";
const DEFAULT_RESOURCE_TTL: Duration = Duration::from_secs(60);
const DEFAULT_RESOURCE_TEMPLATE_TTL: Duration = Duration::from_secs(120);
const MCP_BLOB_DIR: &str = ".talos/mcp_blobs";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_token: Option<String>,
    },
}

/// Build the qualified tool name for an MCP tool.
pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    format!("mcp{}{}{}{}", MCP_DELIMITER, server, MCP_DELIMITER, tool)
}

/// Parse a qualified MCP tool name into (server, tool).
///
/// Returns `None` if the name does not follow the `mcp__<server>__<tool>` pattern.
pub fn split_mcp_tool_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix(MCP_TOOL_NAME_PREFIX)?;
    let mut parts = rest.splitn(2, MCP_DELIMITER);
    let server = parts.next()?.to_string();
    let tool = parts.next()?.to_string();
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

/// Manages connections to one or more MCP servers.
///
/// Each server is connected on first use via [`McpConnectionManager::connect`].
/// After connecting, tool schemas can be listed and tools can be invoked through
/// the typed interface.
pub struct McpConnectionManager {
    /// server_name → client
    clients: HashMap<String, Arc<McpClient>>,
    /// server_name → cached tool definitions
    tool_specs: HashMap<String, Vec<RegisteredToolSpec>>,
    resource_specs: Vec<RegisteredToolSpec>,
    resource_cache: RwLock<McpResourceCache>,
}

#[derive(Debug)]
struct McpResourceCache {
    resources: HashMap<String, CachedEntry<serde_json::Value>>,
    templates: HashMap<String, CachedEntry<serde_json::Value>>,
    resource_ttl: Duration,
    template_ttl: Duration,
}

#[derive(Debug)]
struct CachedEntry<T> {
    value: T,
    cached_at: Instant,
    ttl: Duration,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct McpReadResourceResponse {
    server: String,
    uri: String,
    contents: Vec<McpReadResourceEntry>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum McpReadResourceEntry {
    Text {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
    Blob {
        uri: String,
        mime_type: Option<String>,
        path: String,
        sha256: String,
        size_bytes: u64,
        encoding: &'static str,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct PersistedBlobArtifact {
    path: PathBuf,
    sha256: String,
    size_bytes: u64,
}

impl<T> CachedEntry<T> {
    fn is_fresh(&self) -> bool {
        self.cached_at.elapsed() < self.ttl
    }
}

impl Default for McpResourceCache {
    fn default() -> Self {
        Self {
            resources: HashMap::new(),
            templates: HashMap::new(),
            resource_ttl: DEFAULT_RESOURCE_TTL,
            template_ttl: DEFAULT_RESOURCE_TEMPLATE_TTL,
        }
    }
}

impl McpConnectionManager {
    /// Create an empty manager with no server connections.
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            tool_specs: HashMap::new(),
            resource_specs: Self::resource_tool_specs(),
            resource_cache: RwLock::new(McpResourceCache::default()),
        }
    }

    /// Connect to a single MCP server and cache its tool catalog.
    pub async fn connect(&mut self, config: &McpServerConfig) -> TalosResult<()> {
        let transport: Arc<dyn McpTransport> = match &config.transport {
            McpTransportConfig::Stdio { command, args, env } => {
                Arc::new(StdioTransport::spawn(command, args, env).await?)
            }
            McpTransportConfig::Http { url, bearer_token } => {
                Arc::new(HttpTransport::new(url.clone(), bearer_token.clone()))
            }
        };

        let client = Arc::new(McpClient::new(transport));
        client.initialize().await?;

        let tools = client.list_tools().await?;
        let specs: Vec<RegisteredToolSpec> = tools
            .into_iter()
            .map(|t| {
                let qualified_name = mcp_tool_name(&config.name, &t.name);
                let definition = ToolDefinition {
                    name: qualified_name,
                    description: t.description.unwrap_or_default(),
                    parameters: Some(
                        t.input_schema
                            .unwrap_or(serde_json::json!({"type":"object","properties":{}})),
                    ),
                };
                RegisteredToolSpec::new(
                    definition,
                    ToolSource::Mcp,
                    ToolPayloadKind::Mcp,
                    // MCP tools are treated as parallel-safe by default; individual
                    // servers can override by declaring mutating tools.
                    ToolParallelism::ParallelSafe,
                    false,
                    default_governance_for_source(ToolSource::Mcp),
                )
            })
            .collect();

        self.tool_specs.insert(config.name.clone(), specs);
        self.clients.insert(config.name.clone(), client);
        self.invalidate_server_cache(config.name.as_str()).await;
        Ok(())
    }

    /// Connect to all servers described in a slice of configs.
    pub async fn connect_all(&mut self, configs: &[McpServerConfig]) -> TalosResult<()> {
        for cfg in configs {
            self.connect(cfg).await?;
        }
        Ok(())
    }

    /// Return all registered tool specs across every connected server.
    pub fn all_tool_specs(&self) -> Vec<RegisteredToolSpec> {
        self.tool_specs
            .values()
            .flatten()
            .cloned()
            .chain(self.resource_specs.iter().cloned())
            .collect()
    }

    /// Invoke a tool on the named server.
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Option<Value>,
    ) -> TalosResult<ToolOutputPayload> {
        let client = self.clients.get(server).ok_or_else(|| {
            TalosError::ToolError(format!("MCP server `{}` not connected", server))
        })?;
        let result = client.call_tool(tool, arguments).await?;
        let text = result.to_text();
        Ok(ToolOutputPayload::Mcp {
            content: text,
            is_error: result.is_error,
        })
    }

    pub async fn list_resources(
        &self,
        server: Option<&str>,
        cursor: Option<String>,
    ) -> TalosResult<serde_json::Value> {
        if server.is_none() && cursor.is_some() {
            return Err(TalosError::ValidationError(
                "cursor can only be used when a server is specified".to_string(),
            ));
        }
        let cache_key = cache_key(server, cursor.as_deref());
        if let Some(cached) = self.cached_resources(cache_key.as_str()).await {
            return Ok(cached);
        }
        match server {
            Some(server_name) => {
                let client = self.clients.get(server_name).ok_or_else(|| {
                    TalosError::ToolError(format!("MCP server `{}` not connected", server_name))
                })?;
                let result = client.list_resources(cursor).await?;
                let payload = serde_json::to_value(serde_json::json!({
                    "server": server_name,
                    "resources": result.resources,
                    "next_cursor": result.next_cursor,
                }))
                .map_err(|err| TalosError::ToolError(format!("serialize MCP resources: {err}")))?;
                self.store_resources_cache(cache_key, payload.clone()).await;
                Ok(payload)
            }
            None => {
                let mut resources = Vec::new();
                for (server_name, client) in &self.clients {
                    let result = client.list_resources(None).await?;
                    resources.push(serde_json::json!({
                        "server": server_name,
                        "resources": result.resources,
                    }));
                }
                let payload = serde_json::json!({ "servers": resources });
                self.store_resources_cache(cache_key, payload.clone()).await;
                Ok(payload)
            }
        }
    }

    pub async fn list_resource_templates(
        &self,
        server: Option<&str>,
        cursor: Option<String>,
    ) -> TalosResult<serde_json::Value> {
        if server.is_none() && cursor.is_some() {
            return Err(TalosError::ValidationError(
                "cursor can only be used when a server is specified".to_string(),
            ));
        }
        let cache_key = cache_key(server, cursor.as_deref());
        if let Some(cached) = self.cached_templates(cache_key.as_str()).await {
            return Ok(cached);
        }
        match server {
            Some(server_name) => {
                let client = self.clients.get(server_name).ok_or_else(|| {
                    TalosError::ToolError(format!("MCP server `{}` not connected", server_name))
                })?;
                let result = client.list_resource_templates(cursor).await?;
                let payload = serde_json::to_value(serde_json::json!({
                    "server": server_name,
                    "resource_templates": result.resource_templates,
                    "next_cursor": result.next_cursor,
                }))
                .map_err(|err| {
                    TalosError::ToolError(format!("serialize MCP resource templates: {err}"))
                })?;
                self.store_templates_cache(cache_key, payload.clone()).await;
                Ok(payload)
            }
            None => {
                let mut templates = Vec::new();
                for (server_name, client) in &self.clients {
                    let result = client.list_resource_templates(None).await?;
                    templates.push(serde_json::json!({
                        "server": server_name,
                        "resource_templates": result.resource_templates,
                    }));
                }
                let payload = serde_json::json!({ "servers": templates });
                self.store_templates_cache(cache_key, payload.clone()).await;
                Ok(payload)
            }
        }
    }

    pub async fn read_resource(
        &self,
        server: &str,
        uri: &str,
        context: &ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let client = self.clients.get(server).ok_or_else(|| {
            TalosError::ToolError(format!("MCP server `{}` not connected", server))
        })?;
        let result = client.read_resource(uri).await?;
        build_read_resource_payload(server, uri, result.contents, context).await
    }

    fn resource_tool_specs() -> Vec<RegisteredToolSpec> {
        vec![
            RegisteredToolSpec::new(
                ToolDefinition::new(
                    "list_mcp_resources",
                    "List available MCP resources from one server or all connected servers.",
                    Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "server": { "type": "string" },
                            "cursor": { "type": "string" }
                        }
                    })),
                ),
                ToolSource::Mcp,
                ToolPayloadKind::Function,
                ToolParallelism::ParallelSafe,
                false,
                default_governance_for_source(ToolSource::Mcp),
            ),
            RegisteredToolSpec::new(
                ToolDefinition::new(
                    "list_mcp_resource_templates",
                    "List MCP resource templates from one server or all connected servers.",
                    Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "server": { "type": "string" },
                            "cursor": { "type": "string" }
                        }
                    })),
                ),
                ToolSource::Mcp,
                ToolPayloadKind::Function,
                ToolParallelism::ParallelSafe,
                false,
                default_governance_for_source(ToolSource::Mcp),
            ),
            RegisteredToolSpec::new(
                ToolDefinition::new(
                    "read_mcp_resource",
                    "Read a specific MCP resource from a connected server.",
                    Some(serde_json::json!({
                        "type": "object",
                        "required": ["server", "uri"],
                        "properties": {
                            "server": { "type": "string" },
                            "uri": { "type": "string" }
                        }
                    })),
                ),
                ToolSource::Mcp,
                ToolPayloadKind::Function,
                ToolParallelism::ParallelSafe,
                false,
                default_governance_for_source(ToolSource::Mcp),
            ),
        ]
    }

    async fn cached_resources(&self, key: &str) -> Option<serde_json::Value> {
        let cache = self.resource_cache.read().await;
        cache.resources.get(key).and_then(|entry| {
            if entry.is_fresh() {
                Some(entry.value.clone())
            } else {
                None
            }
        })
    }

    async fn cached_templates(&self, key: &str) -> Option<serde_json::Value> {
        let cache = self.resource_cache.read().await;
        cache.templates.get(key).and_then(|entry| {
            if entry.is_fresh() {
                Some(entry.value.clone())
            } else {
                None
            }
        })
    }

    async fn store_resources_cache(&self, key: String, value: serde_json::Value) {
        let mut cache = self.resource_cache.write().await;
        let ttl = cache.resource_ttl;
        cache.resources.insert(
            key,
            CachedEntry {
                value,
                cached_at: Instant::now(),
                ttl,
            },
        );
    }

    async fn store_templates_cache(&self, key: String, value: serde_json::Value) {
        let mut cache = self.resource_cache.write().await;
        let ttl = cache.template_ttl;
        cache.templates.insert(
            key,
            CachedEntry {
                value,
                cached_at: Instant::now(),
                ttl,
            },
        );
    }

    async fn invalidate_server_cache(&self, server_name: &str) {
        let mut cache = self.resource_cache.write().await;
        cache
            .resources
            .retain(|key, _| !key.starts_with(format!("server:{server_name}|").as_str()));
        cache
            .templates
            .retain(|key, _| !key.starts_with(format!("server:{server_name}|").as_str()));
        cache.resources.remove("all|cursor:");
        cache.templates.remove("all|cursor:");
    }
}

impl Default for McpConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn build_read_resource_payload(
    server: &str,
    uri: &str,
    contents: Vec<protocol::McpResourceContents>,
    context: &ToolContext,
) -> TalosResult<ToolOutputPayload> {
    let mut entries = Vec::new();
    for (index, item) in contents.into_iter().enumerate() {
        match item {
            protocol::McpResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
            } => entries.push(McpReadResourceEntry::Text {
                uri,
                mime_type,
                text,
            }),
            protocol::McpResourceContents::BlobResourceContents {
                uri,
                mime_type,
                blob,
            } => {
                let artifact = persist_blob_artifact(
                    context,
                    server,
                    uri.as_str(),
                    index,
                    mime_type.as_deref(),
                    &blob,
                )
                .await?;
                entries.push(McpReadResourceEntry::Blob {
                    uri,
                    mime_type,
                    path: artifact.path.display().to_string(),
                    sha256: artifact.sha256,
                    size_bytes: artifact.size_bytes,
                    encoding: "binary",
                });
            }
            protocol::McpResourceContents::Unknown => {}
        }
    }

    let payload = McpReadResourceResponse {
        server: server.to_string(),
        uri: uri.to_string(),
        contents: entries,
    };
    let structured_content = serde_json::to_value(&payload)
        .map_err(|err| TalosError::ToolError(format!("serialize MCP resource payload: {err}")))?;
    Ok(ToolOutputPayload::Function {
        content: structured_content.to_string(),
        success: true,
        structured_content: Some(structured_content),
    })
}

async fn persist_blob_artifact(
    context: &ToolContext,
    server: &str,
    uri: &str,
    index: usize,
    mime_type: Option<&str>,
    blob: &str,
) -> TalosResult<PersistedBlobArtifact> {
    let bytes = decode_base64_blob(blob)?;
    let sha256 = sha256_hex(&bytes);
    let directory = blob_storage_dir(context, server);
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|err| TalosError::ToolError(format!("create MCP blob directory: {err}")))?;
    let file_name = format!(
        "{}-{}-{}.{}",
        sanitize_path_component(uri),
        index,
        &sha256[..12],
        extension_for_mime_type(mime_type)
    );
    let path = directory.join(file_name);
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|err| TalosError::ToolError(format!("persist MCP blob artifact: {err}")))?;
    Ok(PersistedBlobArtifact {
        path,
        sha256,
        size_bytes: bytes.len() as u64,
    })
}

fn blob_storage_dir(context: &ToolContext, server: &str) -> PathBuf {
    let root = context
        .project_root()
        .or_else(|| {
            context
                .kernel_config()
                .map(|config| PathBuf::from(config.memory.workspace_dir.trim()))
                .filter(|path| !path.as_os_str().is_empty())
        })
        .unwrap_or_else(|| std::env::temp_dir().join("talos"));
    root.join(MCP_BLOB_DIR)
        .join(sanitize_path_component(server))
}

fn extension_for_mime_type(mime_type: Option<&str>) -> &'static str {
    match mime_type {
        Some("application/json") => "json",
        Some("text/plain") => "txt",
        Some("text/html") => "html",
        Some("image/png") => "png",
        Some("image/jpeg") => "jpg",
        Some("image/gif") => "gif",
        Some("application/pdf") => "pdf",
        _ => "bin",
    }
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '_',
        })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "resource".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

fn decode_base64_blob(blob: &str) -> TalosResult<Vec<u8>> {
    let normalized: Vec<u8> = blob
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();

    if normalized.is_empty() {
        return Ok(Vec::new());
    }

    if normalized.len() % 4 != 0 {
        return Err(TalosError::ToolError(
            "invalid MCP blob encoding: base64 payload length must be a multiple of 4".to_string(),
        ));
    }

    let mut bytes = Vec::with_capacity(normalized.len() / 4 * 3);
    for chunk in normalized.chunks(4) {
        let mut values = [0u8; 4];
        let mut padding = 0usize;

        for (index, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                padding += 1;
                values[index] = 0;
                continue;
            }

            if padding > 0 {
                return Err(TalosError::ToolError(
                    "invalid MCP blob encoding: '=' padding must be trailing".to_string(),
                ));
            }

            values[index] = decode_base64_byte(*byte)?;
        }

        let combined = ((values[0] as u32) << 18)
            | ((values[1] as u32) << 12)
            | ((values[2] as u32) << 6)
            | (values[3] as u32);
        bytes.push(((combined >> 16) & 0xff) as u8);
        if padding < 2 {
            bytes.push(((combined >> 8) & 0xff) as u8);
        }
        if padding == 0 {
            bytes.push((combined & 0xff) as u8);
        }
    }
    Ok(bytes)
}

fn decode_base64_byte(byte: u8) -> TalosResult<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(TalosError::ToolError(format!(
            "invalid MCP blob encoding: unsupported base64 byte `{}`",
            byte as char
        ))),
    }
}

fn cache_key(server: Option<&str>, cursor: Option<&str>) -> String {
    match server {
        Some(server_name) => format!("server:{server_name}|cursor:{}", cursor.unwrap_or_default()),
        None => "all|cursor:".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[test]
    fn test_mcp_tool_name_roundtrip() {
        let name = mcp_tool_name("github", "create_issue");
        assert_eq!(name, "mcp__github__create_issue");

        let (server, tool) = split_mcp_tool_name(&name).unwrap();
        assert_eq!(server, "github");
        assert_eq!(tool, "create_issue");
    }

    #[test]
    fn test_split_mcp_tool_name_rejects_non_mcp() {
        assert!(split_mcp_tool_name("bash").is_none());
        assert!(split_mcp_tool_name("mcp__").is_none());
        assert!(split_mcp_tool_name("mcp__server__").is_none());
    }

    #[test]
    fn test_split_mcp_tool_name_with_underscores_in_tool() {
        let name = "mcp__myserver__do_something_cool";
        let (server, tool) = split_mcp_tool_name(name).unwrap();
        assert_eq!(server, "myserver");
        assert_eq!(tool, "do_something_cool");
    }

    #[tokio::test]
    async fn test_connect_all_fails_fast_when_server_connection_fails() {
        let mut manager = McpConnectionManager::new();
        let configs = vec![McpServerConfig {
            name: "broken".to_string(),
            transport: McpTransportConfig::Stdio {
                command: "__talos_missing_mcp_binary__".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
            },
        }];

        let error = manager
            .connect_all(&configs)
            .await
            .expect_err("connect_all should fail when a configured server cannot start");

        assert!(!manager
            .all_tool_specs()
            .iter()
            .any(|spec| spec.definition.name.starts_with("mcp__broken__")));
        assert!(error.to_string().contains("__talos_missing_mcp_binary__"));
    }

    #[test]
    fn test_decode_base64_blob_rejects_invalid_padding() {
        let error = decode_base64_blob("ab=c").expect_err("invalid base64 should fail");
        assert!(error.to_string().contains("padding must be trailing"));
    }

    #[test]
    fn test_decode_base64_blob_success() {
        let bytes = decode_base64_blob("aGVsbG8=").expect("base64 should decode");
        assert_eq!(bytes, b"hello");
    }

    #[tokio::test]
    async fn test_build_read_resource_payload_persists_blob_artifact() {
        let tempdir = tempdir().expect("tempdir should create");
        let context = ToolContext::new("session-1", "user-1").with_project_root(tempdir.path());
        let payload = build_read_resource_payload(
            "github",
            "artifact://demo",
            vec![
                protocol::McpResourceContents::TextResourceContents {
                    uri: "artifact://demo".to_string(),
                    mime_type: Some("text/plain".to_string()),
                    text: "metadata".to_string(),
                },
                protocol::McpResourceContents::BlobResourceContents {
                    uri: "artifact://demo/blob".to_string(),
                    mime_type: Some("application/pdf".to_string()),
                    blob: "aGVsbG8=".to_string(),
                },
            ],
            &context,
        )
        .await
        .expect("payload should build");

        let ToolOutputPayload::Function {
            content,
            success,
            structured_content,
        } = payload
        else {
            panic!("expected function payload");
        };

        assert!(success);
        assert!(!content.contains("aGVsbG8="));

        let structured = structured_content.expect("structured content should exist");
        let blob = structured["contents"]
            .as_array()
            .and_then(|items| items.get(1))
            .expect("blob entry should exist");
        let blob_path = blob["path"].as_str().expect("blob path should be a string");
        let persisted = fs::read(blob_path).await.expect("blob file should exist");
        assert_eq!(persisted, b"hello");
    }
}
