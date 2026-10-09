//! Talos Configuration
//!
//! 基于 YAML 的配置管理
//!
//! # 配置文件
//!
//! 默认加载位置：
//! 1. `<talos_home>/conf/talos.yaml`
//!
//! # 环境变量覆盖
//!
//! 使用 `${VAR_NAME}` 语法：
//! Environment placeholders are expanded in string values.
//!
//! # 使用方式
//!
//! ```rust,no_run
//! use talos_config::Config;
//!
//! let config = Config::load().unwrap();
//! println!("Agent name: {}", config.agent.name);
//! ```

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use talos_error::TalosResult;
use talos_logging::LoggingConfig;
use ts_rs::TS;

pub mod domains;
pub mod env;
pub mod loader;
pub mod paths;
pub mod store;
pub mod upgrade;
pub mod validate;
pub mod workspace;

pub type TalosTimezone = chrono_tz::Tz;

/// Selects the instance deployment boundary used for configuration access.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(rename_all = "lowercase")]
pub enum DeploymentMode {
    /// A user-owned Talos instance.
    #[default]
    Local,
    /// A shared Talos Cloud instance.
    Cloud,
}

pub use domains::agent::{
    AgentConfig, AgentDefinitionConfig, AgentProviderConfig, MessageQueueConfig, PlannerMode,
};
pub use domains::agent_node::{AgentNodeConfig, AgentNodeControlConfig, AgentNodeRunnerConfig};
pub use domains::automation::{
    AutomationConfig, DreamMemoryMaintenanceConfig, ProfileUsageRetentionConfig,
};
pub use domains::cloud::{CloudAuthConfig, CloudConfig};
pub use domains::fs::{DirectoryPickerConfig, DirectoryPickerRoot, FsConfig};
pub use domains::kernel::{
    AgentKernelCompactionConfig, AgentKernelConfig, AgentKernelOutputConfig,
};
pub use domains::memory::MemoryConfig;
pub use domains::network::{
    allowed_llm_protocols, default_llm_protocol, effective_llm_protocol,
    llm_provider_supports_protocol, provider_protocol_capabilities, ChannelConfig, ChannelsConfig,
    DmPolicy, DmScope, GatewayConfig, GroupPolicy, LlmConfig, LlmProtocol, ModelCostConfig,
    ModelInputKind, ModelRef, ModelSpeed, ProviderConfig, ProviderModelConfig,
    ProviderProtocolCapabilities, ReasoningEffort, TelegramAccountConfig, TelegramChannelsConfig,
    TelegramGroupConfig, TelegramMode, TelegramTopicConfig,
};
pub use domains::plugins::{
    PluginRootConfig, PluginRootKind, PluginSourceConfig, PluginSourceKind, PluginSourceType,
    PluginsRuntimeConfig,
};
pub use domains::prompt::{
    AgentKernelBootstrapPromptConfig, AgentKernelPromptConfig, BootstrapPromptWarningMode,
    PromptWindowPolicy, ReservedOutputPolicy,
};
pub use domains::skills::SkillsRuntimeConfig;
pub use domains::storage::{
    DatabaseType, FileStorageBackendConfig, FileStorageConfig, FileStorageLimits,
    FsFileStorageConfig, S3FileStorageConfig, StorageBackendConfig,
};
pub use domains::tools::{
    AgentKernelMcpConfig, DynamicToolConfig, DynamicToolTimeoutBehavior, FileToolConfig,
    LocalExecToolConfig, McpServerConfig, McpToolConfig, McpTransportConfig, ToolSandboxModeConfig,
    ToolSandboxNetworkConfig, ToolSandboxPlatformConfig, ToolSandboxPolicy,
    ToolUserPermissionModeConfig, ToolsRuntimeConfig, WebSearchProviderConfig,
    WebSearchProviderRuntimeConfig, WebSearchToolConfig,
};
pub use env::{EnvSecretStore, RuntimeEnv};
pub use loader::{install_database_migrations, DatabaseMigrationsInstallReport};
pub use paths::{
    resolve_runtime_path, resolve_talos_home, sanitize_agent_name, set_talos_home, TalosPaths,
};
pub use store::{
    resolve_llm_update_for_runtime, resolve_web_search_update_for_runtime, ConfigStore,
    InMemoryLlmConfigStore, LlmConfigStore, YamlConfigStore, YamlLlmConfigStore,
};
pub use upgrade::{
    refresh_default_config_template, upgrade_default_config, ConfigTemplateRefreshReport,
    ConfigUpgradeReport,
};
pub use workspace::{
    validate_session_workspace_user, validate_user_workspace_segment, EffectiveWorkspace,
    WorkspaceConfig, WorkspaceDefinition, DEFAULT_WORKSPACE_ID,
};

/// 主配置结构
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(skip)]
    source_path: Option<PathBuf>,

    #[serde(skip)]
    runtime_paths: Option<TalosPaths>,

    /// Controls access to instance-level configuration.
    #[serde(default)]
    pub mode: DeploymentMode,

    /// Global timezone used for user-visible time rendering.
    #[serde(default = "default_timezone")]
    pub timezone: String,

    /// Scheduled maintenance configuration.
    #[serde(default)]
    pub automation: AutomationConfig,

    /// Long-term memory file layout and file-list limits.
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Product-managed logical Workspace definitions and default routing.
    #[serde(default)]
    pub workspaces: WorkspaceConfig,

    /// Agent 配置
    #[serde(default)]
    pub agent: AgentConfig,

    /// Distributed Agent node control and runner configuration.
    #[serde(default)]
    pub agent_node: AgentNodeConfig,

    /// Gateway 配置
    #[serde(default)]
    pub gateway: GatewayConfig,

    /// Cloud control-plane configuration.
    #[serde(default)]
    pub cloud: CloudConfig,

    /// Filesystem-facing UI capabilities.
    #[serde(default)]
    pub fs: FsConfig,

    /// 渠道配置
    #[serde(default)]
    pub channels: ChannelsConfig,

    /// 日志配置
    #[serde(default)]
    pub logging: LoggingConfig,

    /// Storage backend configuration
    #[serde(default)]
    pub storage: StorageBackendConfig,

    /// LLM 配置
    #[serde(default)]
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ModelReadiness {
    pub ready: bool,
    pub provider: String,
    pub model: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ProviderKeyStatus {
    pub api_key_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfigRead {
    pub config: LlmConfig,
    pub model_readiness: ModelReadiness,
    pub provider_keys: BTreeMap<String, ProviderKeyStatus>,
}

impl Default for Config {
    fn default() -> Self {
        let mut config = Self {
            source_path: None,
            runtime_paths: None,
            mode: DeploymentMode::default(),
            timezone: default_timezone(),
            automation: AutomationConfig::default(),
            memory: MemoryConfig::default(),
            workspaces: WorkspaceConfig::default(),
            agent: AgentConfig::default(),
            agent_node: AgentNodeConfig::default(),
            gateway: GatewayConfig::default(),
            cloud: CloudConfig::default(),
            fs: FsConfig::default(),
            channels: ChannelsConfig::default(),
            logging: LoggingConfig::default(),
            storage: StorageBackendConfig::default(),
            llm: LlmConfig::default(),
        };
        config.apply_derived_paths();
        config
    }
}

pub fn default_timezone() -> String {
    "UTC".to_string()
}

pub fn parse_timezone(input: &str) -> TalosResult<TalosTimezone> {
    input.trim().parse::<TalosTimezone>().map_err(|err| {
        talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid timezone `{}`: {}", input.trim(), err),
        )
    })
}

impl Config {
    /// Attach runtime-only environment values loaded by the configuration layer.
    pub fn attach_runtime_env(&mut self, runtime_env: RuntimeEnv) {
        self.agent.kernel.runtime_env = runtime_env;
    }

    pub fn talos_paths(&self) -> TalosPaths {
        self.runtime_paths
            .clone()
            .unwrap_or_else(TalosPaths::resolve)
    }

    pub fn default_workspace_id(&self) -> &str {
        &self.workspaces.default_workspace_id
    }

    pub fn resolve_workspace(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> TalosResult<EffectiveWorkspace> {
        self.workspaces
            .resolve(&self.talos_paths(), workspace_id, user_id)
    }

    pub fn workspace_root_prefix(&self, workspace_id: &str) -> TalosResult<PathBuf> {
        self.workspaces
            .root_prefix(&self.talos_paths(), workspace_id)
    }

    /// Replace configured Workspace definitions for an embedded runtime or test.
    pub fn with_workspace_definition(mut self, definition: WorkspaceDefinition) -> Self {
        self.workspaces = WorkspaceConfig {
            default_workspace_id: definition.workspace_id.clone(),
            definitions: vec![definition],
        };
        self
    }

    /// Override runtime filesystem roots for an embedded runtime or test.
    pub fn with_runtime_paths(mut self, paths: TalosPaths) -> Self {
        self.runtime_paths = Some(paths);
        self
    }

    pub fn apply_derived_paths(&mut self) {
        let paths = self.talos_paths();
        self.logging.file = Some(paths.log_file.to_string_lossy().to_string());
        self.storage.connection_string = paths.sqlite_db.to_string_lossy().to_string();
        for backend in self.storage.files.backends.values_mut() {
            let FileStorageBackendConfig::Fs(config) = backend else {
                continue;
            };
            let root_text = config.root.to_string_lossy();
            if root_text.trim().is_empty() || root_text.contains("${") {
                continue;
            }
            let root = resolve_runtime_path(root_text.as_ref());
            config.root = if root.is_absolute() {
                root
            } else {
                paths.data_dir.join(root)
            };
        }
        self.agent.kernel.timezone = self.timezone.clone();
    }

    pub fn current_provider_config(&self) -> Option<&ProviderConfig> {
        match self.llm.provider.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(&self.llm.anthropic),
            "openai" => Some(&self.llm.openai),
            "bailian" => Some(&self.llm.bailian),
            "deepseek" => Some(&self.llm.deepseek),
            "ollama" => Some(&self.llm.ollama),
            "openrouter" => Some(&self.llm.openrouter),
            _ => None,
        }
    }

    pub fn provider_config(&self, provider: &str) -> Option<&ProviderConfig> {
        match provider.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(&self.llm.anthropic),
            "openai" => Some(&self.llm.openai),
            "bailian" => Some(&self.llm.bailian),
            "deepseek" => Some(&self.llm.deepseek),
            "ollama" => Some(&self.llm.ollama),
            "openrouter" => Some(&self.llm.openrouter),
            _ => None,
        }
    }

    /// Load configuration.
    ///
    /// Lookup path:
    /// 1. Explicit path passed by caller
    /// 2. `TALOS_CONFIG`
    /// 3. `<talos_home>/conf/talos.yaml`
    pub fn load() -> TalosResult<Self> {
        loader::load_config()
    }

    /// Load configuration from a specific path.
    pub fn load_from_path(path: Option<&str>) -> TalosResult<Self> {
        loader::load_config_from_path(path)
    }

    /// 保存到文件
    pub fn save(&self, path: &str) -> TalosResult<()> {
        loader::save_config(self, path)
    }

    /// 获取配置文件路径
    pub fn default_path() -> PathBuf {
        loader::default_config_path()
    }

    /// Reload this instance's source and environment without switching runtime home.
    pub fn reload(&self) -> TalosResult<Self> {
        loader::load_config_at(self.source_path(), self.talos_paths())
    }

    /// Return the file that produced this configuration instance.
    pub fn source_path(&self) -> PathBuf {
        self.source_path
            .clone()
            .unwrap_or_else(|| self.talos_paths().config_file)
    }

    #[cfg(test)]
    fn expand_env_in_string(input: &str) -> String {
        loader::expand_env_in_string(input)
    }

    /// Validate runtime-critical configuration values.
    pub fn validate_runtime(&self) -> TalosResult<()> {
        validate::validate_runtime_config(self)
    }

    pub fn model_readiness(&self) -> ModelReadiness {
        model_readiness_for_llm(&self.llm)
    }
}

pub fn model_readiness_for_llm(llm: &LlmConfig) -> ModelReadiness {
    let provider_name = llm.provider.clone();
    let mut issues = Vec::new();

    let Some(provider) = provider_config_for_llm(llm, &provider_name) else {
        issues.push(format!("unsupported llm.provider: {}", provider_name));
        return ModelReadiness {
            ready: false,
            provider: provider_name,
            model: llm.model.clone(),
            issues,
        };
    };

    if let Some(protocol) = provider.protocol {
        if !llm_provider_supports_protocol(&provider_name, protocol) {
            issues.push(format!(
                "llm.{}.protocol '{}' is not supported",
                provider_name, protocol
            ));
        }
    }

    if provider_name != "ollama" {
        match provider.api_key.as_ref() {
            Some(api_key)
                if !api_key.trim().is_empty() && !validate::is_unresolved_placeholder(api_key) => {}
            Some(_) => issues.push(format!(
                "llm.{}.api_key is empty or unresolved placeholder",
                provider_name
            )),
            None => issues.push(format!("llm.{}.api_key is required", provider_name)),
        }
    }

    match provider.base_url.as_ref() {
        Some(base_url)
            if base_url.trim().is_empty() || validate::is_unresolved_placeholder(base_url) =>
        {
            issues.push(format!(
                "llm.{}.base_url is empty or unresolved placeholder",
                provider_name
            ));
        }
        None if provider_name == "bailian" || provider_name == "ollama" => {
            issues.push(format!("llm.{}.base_url is required", provider_name));
        }
        _ => {}
    }

    if llm.model.trim().is_empty() || validate::is_unresolved_placeholder(&llm.model) {
        issues.push("llm.model is empty or unresolved placeholder".to_string());
    } else if !provider.models.iter().any(|model| model.id == llm.model) {
        issues.push(format!(
            "llm.model '{}' is not configured under llm.{}.models",
            llm.model, provider_name
        ));
    }

    ModelReadiness {
        ready: issues.is_empty(),
        provider: provider_name,
        model: llm.model.clone(),
        issues,
    }
}

pub fn provider_config_for_llm<'a>(
    llm: &'a LlmConfig,
    provider: &str,
) -> Option<&'a ProviderConfig> {
    match provider {
        "anthropic" => Some(&llm.anthropic),
        "openai" => Some(&llm.openai),
        "bailian" => Some(&llm.bailian),
        "deepseek" => Some(&llm.deepseek),
        "ollama" => Some(&llm.ollama),
        "openrouter" => Some(&llm.openrouter),
        _ => None,
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

pub use domains::mcp_auth::{McpAuthentication, McpCredentialBinding, McpOAuthConfig};
