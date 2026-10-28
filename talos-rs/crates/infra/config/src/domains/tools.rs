use schemars::JsonSchema;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsRuntimeConfig {
    #[serde(default)]
    pub sandbox_policy: ToolSandboxPolicy,
    #[serde(default)]
    pub bash: BashToolConfig,
    #[serde(default)]
    pub file: FileToolConfig,
    #[serde(default)]
    pub web_search: WebSearchToolConfig,
    #[serde(default)]
    pub mcp_runtime: McpToolConfig,
    #[serde(default)]
    pub dynamic: DynamicToolConfig,
    #[serde(default = "default_true")]
    pub enable_mcp_tools: bool,
    #[serde(default)]
    pub enable_dynamic_tools: bool,
    #[serde(default = "default_true")]
    pub enable_collaboration_tools: bool,
}

impl Default for ToolsRuntimeConfig {
    fn default() -> Self {
        Self {
            sandbox_policy: ToolSandboxPolicy::default(),
            bash: BashToolConfig::default(),
            file: FileToolConfig::default(),
            web_search: WebSearchToolConfig::default(),
            mcp_runtime: McpToolConfig::default(),
            dynamic: DynamicToolConfig::default(),
            enable_mcp_tools: true,
            enable_dynamic_tools: false,
            enable_collaboration_tools: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WebSearchToolConfig {
    #[serde(default)]
    pub provider: WebSearchProviderConfig,
    #[serde(default)]
    pub firecrawl: WebSearchProviderRuntimeConfig,
    #[serde(default)]
    pub brave: WebSearchProviderRuntimeConfig,
    #[serde(default)]
    pub tavily: WebSearchProviderRuntimeConfig,
}

impl Default for WebSearchToolConfig {
    fn default() -> Self {
        Self {
            provider: WebSearchProviderConfig::default(),
            firecrawl: WebSearchProviderRuntimeConfig {
                api_key: Some("${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}".to_string()),
                base_url: Some("https://api.firecrawl.dev".to_string()),
                ..WebSearchProviderRuntimeConfig::default()
            },
            brave: WebSearchProviderRuntimeConfig {
                api_key: Some("${TALOS_WEB_SEARCH_BRAVE_API_KEY}".to_string()),
                base_url: Some("https://api.search.brave.com/res/v1/web/search".to_string()),
                ..WebSearchProviderRuntimeConfig::default()
            },
            tavily: WebSearchProviderRuntimeConfig {
                api_key: Some("${TALOS_WEB_SEARCH_TAVILY_API_KEY}".to_string()),
                base_url: Some("https://api.tavily.com".to_string()),
                ..WebSearchProviderRuntimeConfig::default()
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchProviderConfig {
    #[default]
    Firecrawl,
    Brave,
    Tavily,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WebSearchProviderRuntimeConfig {
    #[serde(default)]
    #[ts(optional)]
    pub api_key: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub base_url: Option<String>,
    #[serde(default = "default_tools_web_search_timeout_ms")]
    #[ts(type = "number")]
    pub timeout_ms: u64,
}

impl Default for WebSearchProviderRuntimeConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            timeout_ms: default_tools_web_search_timeout_ms(),
        }
    }
}

impl Serialize for WebSearchProviderRuntimeConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut field_count = 1;
        if self.api_key.is_some() {
            field_count += 1;
        }
        if self.base_url.is_some() {
            field_count += 1;
        }

        let mut state =
            serializer.serialize_struct("WebSearchProviderRuntimeConfig", field_count)?;
        if let Some(api_key) = &self.api_key {
            state.serialize_field("api_key", api_key)?;
        }
        if let Some(base_url) = &self.base_url {
            state.serialize_field("base_url", base_url)?;
        }
        state.serialize_field("timeout_ms", &self.timeout_ms)?;
        state.end()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSandboxPolicy {
    #[serde(default)]
    pub user_permission_mode: ToolUserPermissionModeConfig,
    #[serde(default)]
    pub default_mode: ToolSandboxModeConfig,
    #[serde(default)]
    pub network: ToolSandboxNetworkConfig,
    #[serde(default)]
    pub platform: ToolSandboxPlatformConfig,
}

impl Default for ToolSandboxPolicy {
    fn default() -> Self {
        Self {
            user_permission_mode: ToolUserPermissionModeConfig::default(),
            default_mode: ToolSandboxModeConfig::UseDefault,
            network: ToolSandboxNetworkConfig::default(),
            platform: ToolSandboxPlatformConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolUserPermissionModeConfig {
    AskForApproval,
    ApproveForMe,
    #[default]
    FullAccess,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolSandboxNetworkConfig {
    #[default]
    UseDefault,
    Restricted,
    Enabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolSandboxPlatformConfig {
    #[serde(default)]
    pub linux_helper_executable: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashToolConfig {
    #[serde(default = "default_kernel_tool_result_max_bytes")]
    pub max_output_bytes: usize,
    #[serde(default = "default_tools_exec_timeout_ms")]
    pub default_timeout_ms: u64,
    #[serde(default)]
    pub output_log: BashOutputLogConfig,
}

/// Finite node-local log retention for one already user-scoped Workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BashOutputLogConfig {
    pub max_file_bytes: u64,
    pub max_workspace_bytes: u64,
    pub retention_days: u32,
    pub cleanup_interval_secs: u64,
}

impl BashOutputLogConfig {
    pub fn validate(&self) -> talos_error::TalosResult<()> {
        if self.max_file_bytes == 0
            || self.retention_days == 0
            || self.cleanup_interval_secs == 0
            || std::time::Instant::now()
                .checked_add(std::time::Duration::from_secs(self.cleanup_interval_secs))
                .is_none()
            || self
                .max_file_bytes
                .checked_add(4096)
                .is_none_or(|reservation| self.max_workspace_bytes < reservation)
        {
            return Err(talos_error::TalosError::new(talos_error::ErrorCode::InvalidRequest,
                "Bash log limits must be positive and the Workspace budget must cover a log and its metadata."));
        }
        Ok(())
    }
}

impl Default for BashOutputLogConfig {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024 * 1024,
            max_workspace_bytes: 5 * 1024 * 1024 * 1024,
            retention_days: 7,
            cleanup_interval_secs: 3600,
        }
    }
}

impl Default for BashToolConfig {
    fn default() -> Self {
        Self {
            max_output_bytes: default_kernel_tool_result_max_bytes(),
            default_timeout_ms: default_tools_exec_timeout_ms(),
            output_log: BashOutputLogConfig::default(),
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

fn default_true() -> bool {
    true
}

fn default_kernel_tool_result_max_bytes() -> usize {
    64 * 1024
}

fn default_tools_exec_timeout_ms() -> u64 {
    60_000
}

fn default_tools_web_search_timeout_ms() -> u64 {
    15_000
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

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
