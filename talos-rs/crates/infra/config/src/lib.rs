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

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use talos_error::TalosResult;
use talos_logging::LoggingConfig;

pub mod domains;
pub mod env;
pub mod loader;
pub mod paths;
pub mod service;
pub mod store;
pub mod validate;
pub mod workspace;

pub type TalosTimezone = chrono_tz::Tz;
pub use domains::agent::{
    AgentConfig, AgentDefinitionConfig, AgentKind, AgentRuntimeConfig, PlannerMode,
};
pub use domains::cloud::{CloudAuthConfig, CloudConfig};
pub use domains::fs::{DirectoryPickerConfig, DirectoryPickerRoot, FsConfig};
pub use domains::kernel::{
    AgentKernelCompactionConfig, AgentKernelConfig, AgentKernelOutputConfig,
    AgentKernelTimelineEventsConfig,
};
pub use domains::memory::AgentKernelMemoryConfig;
pub use domains::network::{
    allowed_llm_protocols, default_llm_protocol, effective_llm_protocol,
    llm_provider_supports_protocol, ChannelConfig, ChannelsConfig, DmPolicy, DmScope,
    GatewayConfig, GroupPolicy, LlmConfig, LlmProtocol, ModelCompatConfig, ModelCostConfig,
    ModelInputKind, ModelSpeed, ProviderConfig, ProviderModelConfig, ReasoningEffort,
    TelegramAccountConfig, TelegramChannelsConfig, TelegramGroupConfig, TelegramMode,
    TelegramTopicConfig, ThinkingFormat,
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
pub use domains::storage::{DatabaseType, StorageBackendConfig};
pub use domains::tools::{
    AgentKernelMcpConfig, DynamicToolConfig, DynamicToolTimeoutBehavior, FileToolConfig,
    LocalExecToolConfig, McpServerConfig, McpToolConfig, McpTransportConfig, ToolSandboxModeConfig,
    ToolSandboxPolicy, ToolsRuntimeConfig, WebSearchProviderConfig, WebSearchProviderRuntimeConfig,
    WebSearchToolConfig,
};
pub use env::{EnvSecretStore, RuntimeEnv};
pub use paths::{
    resolve_runtime_path, resolve_talos_home, sanitize_agent_name, set_talos_home, TalosPaths,
};
pub use service::{LlmConfigManager, LlmConfigService};
pub use store::{InMemoryLlmConfigStore, LlmConfigStore, YamlLlmConfigStore};
pub use workspace::{
    resolve_effective_kernel_workspace, validate_session_workspace_user,
    validate_user_workspace_segment, EffectiveKernelWorkspace,
};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelReadiness {
    pub ready: bool,
    pub provider: String,
    pub model: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            timezone: default_timezone(),
            agent: AgentConfig::default(),
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
        self.agent.kernel.workspace_dir = paths
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
            "openrouter" => Some(&self.llm.openrouter),
            _ => None,
        }
    }

    pub fn provider_config(&self, provider: &str) -> Option<&ProviderConfig> {
        match provider.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(&self.llm.anthropic),
            "openai" => Some(&self.llm.openai),
            "bailian" => Some(&self.llm.bailian),
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

    /// Return the config path that the loader will use for the current process.
    pub fn source_path() -> PathBuf {
        loader::config_source_path(None)
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
    let provider_name = llm.provider.trim().to_ascii_lowercase();
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

    if !provider.enabled {
        issues.push(format!("llm provider '{}' is disabled", provider_name));
    }

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
    } else if !provider.models.is_empty()
        && !provider.models.iter().any(|model| model.id == llm.model)
    {
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
    match provider.trim().to_ascii_lowercase().as_str() {
        "anthropic" => Some(&llm.anthropic),
        "openai" => Some(&llm.openai),
        "bailian" => Some(&llm.bailian),
        "ollama" => Some(&llm.ollama),
        "openrouter" => Some(&llm.openrouter),
        _ => None,
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
