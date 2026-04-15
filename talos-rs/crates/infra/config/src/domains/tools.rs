use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsRuntimeConfig {
    #[serde(default)]
    pub approval_policy: ToolApprovalPolicy,
    #[serde(default)]
    pub sandbox_policy: ToolSandboxPolicy,
    #[serde(default)]
    pub local_exec: LocalExecToolConfig,
    #[serde(default)]
    pub file: FileToolConfig,
    #[serde(default)]
    pub mcp_runtime: McpToolConfig,
    #[serde(default)]
    pub dynamic: DynamicToolConfig,
    #[serde(default = "default_true")]
    pub enable_apply_patch: bool,
    #[serde(default = "default_true")]
    pub enable_mcp_tools: bool,
    #[serde(default)]
    pub enable_dynamic_tools: bool,
    #[serde(default)]
    pub enable_collaboration_tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalPolicy {
    #[default]
    OnRequest,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSandboxPolicy {
    #[serde(default)]
    pub default_mode: ToolSandboxModeConfig,
}

impl Default for ToolSandboxPolicy {
    fn default() -> Self {
        Self {
            default_mode: ToolSandboxModeConfig::UseDefault,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolSandboxModeConfig {
    #[default]
    UseDefault,
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalExecToolConfig {
    #[serde(default = "default_kernel_tool_result_max_bytes")]
    pub max_output_bytes: usize,
    #[serde(default = "default_tools_exec_timeout_ms")]
    pub default_timeout_ms: u64,
}

impl Default for LocalExecToolConfig {
    fn default() -> Self {
        Self {
            max_output_bytes: default_kernel_tool_result_max_bytes(),
            default_timeout_ms: default_tools_exec_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileToolConfig {
    #[serde(default = "default_tools_file_read_limit")]
    pub default_read_limit: usize,
    #[serde(default = "default_tools_file_search_limit")]
    pub default_search_limit: usize,
    #[serde(default = "default_tools_file_list_limit")]
    pub default_list_limit: usize,
}

impl Default for FileToolConfig {
    fn default() -> Self {
        Self {
            default_read_limit: default_tools_file_read_limit(),
            default_search_limit: default_tools_file_search_limit(),
            default_list_limit: default_tools_file_list_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolConfig {
    #[serde(default = "default_true")]
    pub enable_tools: bool,
    #[serde(default = "default_true")]
    pub enable_resource_tools: bool,
}

impl Default for McpToolConfig {
    fn default() -> Self {
        Self {
            enable_tools: true,
            enable_resource_tools: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolConfig {
    #[serde(default = "default_tools_dynamic_timeout_ms")]
    pub response_timeout_ms: u64,
    #[serde(default)]
    pub on_timeout: DynamicToolTimeoutBehavior,
}

impl Default for DynamicToolConfig {
    fn default() -> Self {
        Self {
            response_timeout_ms: default_tools_dynamic_timeout_ms(),
            on_timeout: DynamicToolTimeoutBehavior::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DynamicToolTimeoutBehavior {
    #[default]
    ErrorToModel,
    AbortTurn,
}

/// MCP tool federation configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentKernelMcpConfig {
    /// List of MCP servers to connect on agent startup
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

/// Configuration for a single MCP server connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Logical name used to prefix tool names: `mcp__<name>__<tool>`
    pub name: String,

    /// Transport used to communicate with the server
    pub transport: McpTransportConfig,
}

/// MCP transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransportConfig {
    /// Launch an MCP server as a child process and communicate via stdin/stdout
    Stdio {
        /// Executable to run
        command: String,
        /// Arguments passed to the command
        #[serde(default)]
        args: Vec<String>,
        /// Additional environment variables
        #[serde(default)]
        env: HashMap<String, String>,
    },
    /// Connect to an MCP server over HTTP (streamable JSON-RPC)
    Http {
        /// Base URL of the MCP server (e.g. `http://localhost:3000`)
        url: String,
        /// Optional bearer token for authentication
        #[serde(default)]
        bearer_token: Option<String>,
    },
}

fn default_true() -> bool {
    true
}

fn default_kernel_tool_result_max_bytes() -> usize {
    64 * 1024
}

fn default_tools_exec_timeout_ms() -> u64 {
    60_000
}

fn default_tools_dynamic_timeout_ms() -> u64 {
    300_000
}

fn default_tools_file_read_limit() -> usize {
    200
}

fn default_tools_file_search_limit() -> usize {
    200
}

fn default_tools_file_list_limit() -> usize {
    200
}
