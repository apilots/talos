mod client;
mod protocol;
pub mod transport;

pub use client::McpClient;
pub use protocol::McpToolDefinition;

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use talos_config::McpServerConfig;
use talos_config::McpTransportConfig;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::ToolDefinition;

use crate::mcp::transport::{HttpTransport, McpTransport, StdioTransport};
use crate::output::ToolOutputPayload;
use crate::spec::{RegisteredToolSpec, ToolParallelism, ToolPayloadKind, ToolSource};

/// Prefix used in tool names for MCP tools: `mcp__<server>__<tool>`.
pub const MCP_TOOL_NAME_PREFIX: &str = "mcp__";
const MCP_DELIMITER: &str = "__";

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
}

impl McpConnectionManager {
    /// Create an empty manager with no server connections.
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            tool_specs: HashMap::new(),
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
                    parameters: Some(t.input_schema.unwrap_or(serde_json::json!({"type":"object","properties":{}}))),
                };
                RegisteredToolSpec::new(
                    definition,
                    ToolSource::Mcp,
                    ToolPayloadKind::Mcp,
                    // MCP tools are treated as parallel-safe by default; individual
                    // servers can override by declaring mutating tools.
                    ToolParallelism::ParallelSafe,
                    false,
                )
            })
            .collect();

        self.tool_specs.insert(config.name.clone(), specs);
        self.clients.insert(config.name.clone(), client);
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
        self.tool_specs.values().flatten().cloned().collect()
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
}

impl Default for McpConnectionManager {
    fn default() -> Self {
        Self::new()
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

        assert!(!manager.all_tool_specs().iter().any(|spec| spec.definition.name.starts_with("mcp__broken__")));
        assert!(error.to_string().contains("__talos_missing_mcp_binary__"));
    }
}
