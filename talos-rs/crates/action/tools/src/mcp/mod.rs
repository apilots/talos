mod client;
mod protocol;
pub mod transport;

pub use client::McpClient;
pub use protocol::McpToolDefinition;

use std::collections::HashMap;
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

/// Prefix used in tool names for MCP tools: `mcp__<server>__<tool>`.
pub const MCP_TOOL_NAME_PREFIX: &str = "mcp__";
const MCP_DELIMITER: &str = "__";
const DEFAULT_RESOURCE_TTL: Duration = Duration::from_secs(60);
const DEFAULT_RESOURCE_TEMPLATE_TTL: Duration = Duration::from_secs(120);

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

    pub async fn read_resource(&self, server: &str, uri: &str) -> TalosResult<ToolOutputPayload> {
        let client = self.clients.get(server).ok_or_else(|| {
            TalosError::ToolError(format!("MCP server `{}` not connected", server))
        })?;
        let result = client.read_resource(uri).await?;
        let mut combined = Vec::new();
        let mut mime_type = None;
        for item in result.contents {
            match item {
                protocol::McpResourceContents::TextResourceContents {
                    mime_type: item_mime,
                    text,
                    ..
                } => {
                    if mime_type.is_none() {
                        mime_type = item_mime;
                    }
                    combined.push(text);
                }
                protocol::McpResourceContents::BlobResourceContents {
                    mime_type: item_mime,
                    blob,
                    ..
                } => {
                    if mime_type.is_none() {
                        mime_type = item_mime;
                    }
                    combined.push(blob);
                }
                protocol::McpResourceContents::Unknown => {}
            }
        }
        Ok(ToolOutputPayload::Resource {
            uri: uri.to_string(),
            mime_type,
            content: combined.join("\n"),
        })
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

fn cache_key(server: Option<&str>, cursor: Option<&str>) -> String {
    match server {
        Some(server_name) => format!("server:{server_name}|cursor:{}", cursor.unwrap_or_default()),
        None => "all|cursor:".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(split_mcp_tool_name("exec_command").is_none());
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
}
