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
//! ```yaml
//! gateway:
//!   jwt_secret: ${JWT_SECRET}
//! ```
//!
//! # 使用方式
//!
//! ```rust,no_run
//! use talos_config::Config;
//!
//! let config = Config::load().unwrap();
//! println!("Agent name: {}", config.agent.name);
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use talos_error::TalosResult;
use talos_logging::LoggingConfig;

pub mod domains;
pub mod loader;
pub mod paths;
pub mod service;
pub mod store;
pub mod validate;

pub type TalosTimezone = chrono_tz::Tz;
pub use domains::agent::{
    AgentConfig, ExecutionMode, PlanReActPolicy, PlannerMode, PlannerSelection,
};
pub use domains::kernel::AgentKernelConfig;
pub use domains::memory::AgentKernelMemoryConfig;
pub use domains::network::{
    ChannelConfig, ChannelsConfig, DmPolicy, DmScope, GatewayAuthCacheConfig, GatewayConfig,
    GroupPolicy, LlmConfig, ModelCompatConfig, ModelCostConfig, ModelInputKind, ProviderConfig,
    ProviderModelConfig, TelegramAccountConfig, TelegramChannelsConfig, TelegramGroupConfig,
    TelegramMode, TelegramTopicConfig, ThinkingFormat,
};
pub use domains::prompt::{
    AgentKernelBootstrapPromptConfig, AgentKernelPromptConfig, BootstrapPromptWarningMode,
    PromptWindowPolicy, ReservedOutputPolicy,
};
pub use domains::storage::{DatabaseType, StorageBackendConfig};
pub use domains::tools::{
    AgentKernelMcpConfig, DynamicToolConfig, DynamicToolTimeoutBehavior, FileToolConfig,
    LocalExecToolConfig, McpServerConfig, McpToolConfig, McpTransportConfig, ToolApprovalPolicy,
    ToolSandboxModeConfig, ToolSandboxPolicy, ToolsRuntimeConfig,
};
pub use paths::{
    resolve_runtime_path, resolve_talos_home, sanitize_agent_name, set_talos_home, TalosPaths,
};
pub use service::{LlmConfigManager, LlmConfigService};
pub use store::{InMemoryLlmConfigStore, LlmConfigStore, YamlLlmConfigStore};

/// 主配置结构
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Global timezone used for user-visible time rendering.
    #[serde(default = "default_timezone")]
    pub timezone: String,

    /// Agent 配置
    #[serde(default)]
    pub agent: AgentConfig,

    /// Gateway 配置
    #[serde(default)]
    pub gateway: GatewayConfig,

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

impl Default for Config {
    fn default() -> Self {
        let mut config = Self {
            timezone: default_timezone(),
            agent: AgentConfig::default(),
            gateway: GatewayConfig::default(),
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
        talos_error::TalosError::InvalidConfiguration(format!(
            "invalid timezone `{}`: {}",
            input.trim(),
            err
        ))
    })
}

impl Config {
    pub fn talos_paths(&self) -> TalosPaths {
        TalosPaths::resolve()
    }

    pub fn apply_derived_paths(&mut self) {
        let paths = self.talos_paths();
        self.logging.file = Some(paths.log_file.to_string_lossy().to_string());
        self.storage.connection_string = paths.sqlite_db.to_string_lossy().to_string();
        self.agent.kernel.timezone = self.timezone.clone();
        self.agent.kernel.memory.workspace_dir = paths
            .workspace_dir_for_agent(&self.agent.name)
            .to_string_lossy()
            .to_string();
    }

    pub fn current_provider_config(&self) -> Option<&ProviderConfig> {
        match self.llm.provider.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(&self.llm.anthropic),
            "openai" => Some(&self.llm.openai),
            "bailian" => Some(&self.llm.bailian),
            "ollama" => Some(&self.llm.ollama),
            _ => None,
        }
    }

    pub fn provider_config(&self, provider: &str) -> Option<&ProviderConfig> {
        match provider.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(&self.llm.anthropic),
            "openai" => Some(&self.llm.openai),
            "bailian" => Some(&self.llm.bailian),
            "ollama" => Some(&self.llm.ollama),
            _ => None,
        }
    }

    /// Load configuration.
    ///
    /// Lookup path:
    /// 1. `<talos_home>/conf/talos.yaml`
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

    #[cfg(test)]
    fn expand_env_in_string(input: &str) -> String {
        loader::expand_env_in_string(input)
    }

    /// Validate runtime-critical configuration values.
    pub fn validate_runtime(&self) -> TalosResult<()> {
        validate::validate_runtime_config(self)
    }
}

/// 生成默认配置文件
pub fn generate_default_config() -> String {
    loader::generate_default_config_content()
}

#[cfg(test)]
mod tests;
