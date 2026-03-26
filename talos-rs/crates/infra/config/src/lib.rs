//! Talos Configuration
//!
//! 基于 YAML 的配置管理
//!
//! # 配置文件
//!
//! 默认加载顺序：
//! 1. `~/.talos/talos.yaml` (用户目录)
//! 2. `talos.yaml` (当前目录)
//!
//! # 环境变量覆盖
//!
//! 使用 `${VAR_NAME}` 语法：
//! ```yaml
//! gateway:
//!   token: ${GATEWAY_TOKEN}
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
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use talos_error::TalosResult;
use talos_logging::LoggingConfig;

/// 主配置结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
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
        Self {
            agent: AgentConfig::default(),
            gateway: GatewayConfig::default(),
            channels: ChannelsConfig::default(),
            logging: LoggingConfig::default(),
            storage: StorageBackendConfig::default(),
            llm: LlmConfig::default(),
        }
    }
}

/// Storage backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBackendConfig {
    /// Backend type, currently only `sqlite` is supported.
    #[serde(default = "default_storage_database_type")]
    pub database_type: String,

    /// Database connection string or file path.
    #[serde(default = "default_storage_connection_string")]
    pub connection_string: String,

    /// Connection pool size.
    #[serde(default = "default_storage_max_connections")]
    pub max_connections: u32,
}

fn default_storage_database_type() -> String {
    "sqlite".to_string()
}

fn default_storage_connection_string() -> String {
    "talos.db".to_string()
}

fn default_storage_max_connections() -> u32 {
    5
}

impl Default for StorageBackendConfig {
    fn default() -> Self {
        Self {
            database_type: default_storage_database_type(),
            connection_string: default_storage_connection_string(),
            max_connections: default_storage_max_connections(),
        }
    }
}

/// Agent 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Agent 名称
    #[serde(default = "default_agent_name")]
    pub name: String,

    /// 工作空间路径
    #[serde(default)]
    pub workspace: Option<String>,

    /// 默认模型
    #[serde(default)]
    pub model: String,

    /// 人格文件
    #[serde(default)]
    pub identity: Option<String>,

    /// Agent kernel runtime settings
    #[serde(default)]
    pub kernel: AgentKernelConfig,
}

fn default_agent_name() -> String {
    "talos".to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: default_agent_name(),
            workspace: None,
            model: String::new(),
            identity: None,
            kernel: AgentKernelConfig::default(),
        }
    }
}

/// Agent kernel runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelConfig {
    /// Maximum loop iterations in a single turn
    #[serde(default = "default_kernel_max_iters")]
    pub max_iters: u32,

    /// Maximum tasks allowed in a single turn
    #[serde(default = "default_kernel_max_task_per_turn")]
    pub max_task_per_turn: usize,

    /// Maximum parallel tool executions
    #[serde(default = "default_kernel_tool_parallelism")]
    pub tool_parallelism: usize,

    /// Maximum LLM calls in a turn
    #[serde(default = "default_kernel_max_llm_calls")]
    pub max_llm_calls: u32,

    /// Maximum tool-calling rounds in a general turn
    #[serde(default = "default_kernel_max_tool_rounds")]
    pub max_tool_rounds: u32,

    /// Maximum bytes returned by each tool execution payload
    #[serde(default = "default_kernel_tool_result_max_bytes")]
    pub tool_result_max_bytes: usize,

    /// Trigger context compaction when session memory exceeds this threshold
    #[serde(default = "default_kernel_context_compaction_max_bytes")]
    pub context_compaction_max_bytes: usize,

    /// Default workspace id when request does not provide one
    #[serde(default = "default_workspace_id")]
    pub default_workspace_id: String,

    /// Planner mode for kernel orchestration: built_in or plan_react
    #[serde(default = "default_kernel_planner_mode")]
    pub planner_mode: String,

    /// Deprecated planner override switch; request-level config no longer overrides kernel config.
    #[serde(default = "default_kernel_allow_request_planner_override")]
    pub allow_request_planner_override: bool,

    /// Kernel execution mode: single, sequential, parallel, loop, or a2a.
    #[serde(default = "default_kernel_execution_mode")]
    pub execution_mode: String,

    /// Child agent count used by sequential/parallel execution mode.
    #[serde(default = "default_kernel_execution_children")]
    pub execution_children: usize,

    /// Iteration count used by loop/a2a execution mode.
    #[serde(default = "default_kernel_execution_loop_iters")]
    pub execution_loop_iters: u32,

    /// Prompt builder defaults
    #[serde(default)]
    pub prompt: AgentKernelPromptConfig,

    /// Memory store defaults
    #[serde(default)]
    pub memory: AgentKernelMemoryConfig,

    /// Kernel agent binding defaults
    #[serde(default)]
    pub binding: AgentKernelBindingConfig,
}

fn default_kernel_max_iters() -> u32 {
    1
}

fn default_kernel_max_task_per_turn() -> usize {
    3
}

fn default_kernel_tool_parallelism() -> usize {
    4
}

fn default_kernel_max_llm_calls() -> u32 {
    8
}

fn default_kernel_max_tool_rounds() -> u32 {
    8
}

fn default_kernel_tool_result_max_bytes() -> usize {
    64 * 1024
}

fn default_kernel_context_compaction_max_bytes() -> usize {
    256 * 1024
}

fn default_workspace_id() -> String {
    "default".to_string()
}

fn default_kernel_planner_mode() -> String {
    "built_in".to_string()
}

fn default_kernel_allow_request_planner_override() -> bool {
    false
}

fn default_kernel_execution_mode() -> String {
    "single".to_string()
}

fn default_kernel_execution_children() -> usize {
    2
}

fn default_kernel_execution_loop_iters() -> u32 {
    3
}

/// Prompt defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapPromptWarningMode {
    Off,
    Once,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelBootstrapPromptConfig {
    /// Enable workspace bootstrap prompt injection.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Auto-create missing bootstrap files before reading them.
    #[serde(default = "default_true")]
    pub auto_create_missing_files: bool,

    /// Maximum characters injected per bootstrap file.
    #[serde(default = "default_prompt_bootstrap_max_chars_per_file")]
    pub max_chars_per_file: usize,

    /// Maximum total characters injected across bootstrap files.
    #[serde(default = "default_prompt_bootstrap_max_chars_total")]
    pub max_chars_total: usize,

    /// Whether truncation warnings should be surfaced in diagnostics.
    #[serde(default = "default_bootstrap_prompt_warning_mode")]
    pub warning_mode: BootstrapPromptWarningMode,

    #[serde(default = "default_true")]
    pub include_agents: bool,

    #[serde(default = "default_true")]
    pub include_soul: bool,

    #[serde(default = "default_true")]
    pub include_tools: bool,

    #[serde(default = "default_true")]
    pub include_identity: bool,

    #[serde(default = "default_true")]
    pub include_user: bool,

    #[serde(default = "default_true")]
    pub include_heartbeat: bool,

    #[serde(default = "default_true")]
    pub include_bootstrap: bool,

    #[serde(default = "default_true")]
    pub include_memory: bool,
}

fn default_prompt_bootstrap_max_chars_per_file() -> usize {
    8_000
}

fn default_prompt_bootstrap_max_chars_total() -> usize {
    24_000
}

fn default_bootstrap_prompt_warning_mode() -> BootstrapPromptWarningMode {
    BootstrapPromptWarningMode::Once
}

impl Default for AgentKernelBootstrapPromptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_create_missing_files: true,
            max_chars_per_file: default_prompt_bootstrap_max_chars_per_file(),
            max_chars_total: default_prompt_bootstrap_max_chars_total(),
            warning_mode: default_bootstrap_prompt_warning_mode(),
            include_agents: true,
            include_soul: true,
            include_tools: true,
            include_identity: true,
            include_user: true,
            include_heartbeat: true,
            include_bootstrap: true,
            include_memory: true,
        }
    }
}

/// Prompt defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelPromptConfig {
    /// Static prompt token budget
    #[serde(default = "default_prompt_max_static_tokens")]
    pub max_static_tokens: usize,

    /// Dynamic prompt token budget
    #[serde(default = "default_prompt_max_dynamic_tokens")]
    pub max_dynamic_tokens: usize,

    /// Include session summary in prompt context
    #[serde(default = "default_true")]
    pub include_session_summary: bool,

    /// Include recent messages in prompt context
    #[serde(default = "default_true")]
    pub include_recent_messages: bool,

    /// Include root memory in prompt context
    #[serde(default = "default_true")]
    pub include_root_memory: bool,

    /// Include retrieved memories in prompt context
    #[serde(default = "default_true")]
    pub include_retrieved_memories: bool,

    /// Maximum retrieved memory items injected into prompt
    #[serde(default = "default_prompt_retrieved_memory_max_items")]
    pub retrieved_memory_max_items: usize,

    /// Maximum total chars retained for retrieved memories
    #[serde(default = "default_prompt_retrieved_memory_max_chars")]
    pub retrieved_memory_max_chars: usize,

    /// Workspace bootstrap prompt policy.
    #[serde(default)]
    pub bootstrap: AgentKernelBootstrapPromptConfig,
}

fn default_prompt_max_static_tokens() -> usize {
    1200
}

fn default_prompt_max_dynamic_tokens() -> usize {
    3200
}

impl Default for AgentKernelPromptConfig {
    fn default() -> Self {
        Self {
            max_static_tokens: default_prompt_max_static_tokens(),
            max_dynamic_tokens: default_prompt_max_dynamic_tokens(),
            include_session_summary: true,
            include_recent_messages: true,
            include_root_memory: true,
            include_retrieved_memories: true,
            retrieved_memory_max_items: default_prompt_retrieved_memory_max_items(),
            retrieved_memory_max_chars: default_prompt_retrieved_memory_max_chars(),
            bootstrap: AgentKernelBootstrapPromptConfig::default(),
        }
    }
}

fn default_prompt_retrieved_memory_max_items() -> usize {
    4
}

fn default_prompt_retrieved_memory_max_chars() -> usize {
    2000
}

/// Memory defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelMemoryConfig {
    /// Workspace root directory for memory and session markdown files.
    #[serde(default = "default_kernel_memory_workspace_dir")]
    pub workspace_dir: String,

    /// In-memory hot recent message capacity.
    #[serde(default = "default_kernel_memory_hot_recent_messages")]
    pub hot_recent_messages: usize,

    /// Target message count when building session prompt context.
    #[serde(default = "default_kernel_memory_session_recent_messages_limit")]
    pub session_recent_messages_limit: usize,

    /// Enable session summary injection in prompt context.
    #[serde(default = "default_true")]
    pub session_summary_enabled: bool,

    /// Number of latest summaries to inject.
    #[serde(default = "default_kernel_memory_session_summary_recent_count")]
    pub session_summary_recent_count: usize,

    /// Maximum characters retained per summary item.
    #[serde(default = "default_kernel_memory_session_summary_max_chars")]
    pub session_summary_max_chars: usize,

    /// Trigger summary compaction when session memory exceeds this threshold.
    #[serde(default = "default_kernel_memory_compaction_threshold_bytes")]
    pub compaction_threshold_bytes: usize,

    /// Root memory file name.
    #[serde(default = "default_kernel_memory_root_memory_filename")]
    pub root_memory_filename: String,

    /// Workspace memory directory name.
    #[serde(default = "default_kernel_memory_memory_dirname")]
    pub memory_dirname: String,

    /// Enable workspace memory flush.
    #[serde(default = "default_true")]
    pub workspace_memory_flush_enabled: bool,

    /// Enable workspace memory flush before compaction.
    #[serde(default = "default_true")]
    pub workspace_memory_flush_on_compaction: bool,

    /// Daily workspace memory file pattern.
    #[serde(default = "default_kernel_memory_flush_file_pattern")]
    pub flush_file_pattern: String,

    /// Enable memory search tool.
    #[serde(default = "default_true")]
    pub tool_search_enabled: bool,

    /// Enable memory get tool.
    #[serde(default = "default_true")]
    pub tool_get_enabled: bool,

    /// Default max results for workspace memory search.
    #[serde(default = "default_kernel_memory_search_max_results")]
    pub search_max_results: usize,

    /// Default minimum score for workspace memory search.
    #[serde(default = "default_kernel_memory_search_min_score")]
    pub search_min_score: f32,

    /// Include root memory in workspace memory search.
    #[serde(default = "default_true")]
    pub search_include_root_memory: bool,

    /// Include daily memory files in workspace memory search.
    #[serde(default = "default_true")]
    pub search_include_daily_memory: bool,

    /// Include session transcripts in workspace memory search.
    #[serde(default)]
    pub search_include_sessions: bool,
}

fn default_kernel_memory_workspace_dir() -> String {
    "~/.talos/workspace".to_string()
}

fn default_kernel_memory_hot_recent_messages() -> usize {
    64
}

fn default_kernel_memory_session_recent_messages_limit() -> usize {
    24
}

fn default_kernel_memory_session_summary_recent_count() -> usize {
    2
}

fn default_kernel_memory_session_summary_max_chars() -> usize {
    1200
}

fn default_kernel_memory_compaction_threshold_bytes() -> usize {
    256 * 1024
}

fn default_kernel_memory_root_memory_filename() -> String {
    "MEMORY.md".to_string()
}

fn default_kernel_memory_memory_dirname() -> String {
    "memory".to_string()
}

fn default_kernel_memory_flush_file_pattern() -> String {
    "YYYY-MM-DD.md".to_string()
}

fn default_kernel_memory_search_max_results() -> usize {
    6
}

fn default_kernel_memory_search_min_score() -> f32 {
    0.35
}

impl Default for AgentKernelMemoryConfig {
    fn default() -> Self {
        Self {
            workspace_dir: default_kernel_memory_workspace_dir(),
            hot_recent_messages: default_kernel_memory_hot_recent_messages(),
            session_recent_messages_limit: default_kernel_memory_session_recent_messages_limit(),
            session_summary_enabled: true,
            session_summary_recent_count: default_kernel_memory_session_summary_recent_count(),
            session_summary_max_chars: default_kernel_memory_session_summary_max_chars(),
            compaction_threshold_bytes: default_kernel_memory_compaction_threshold_bytes(),
            root_memory_filename: default_kernel_memory_root_memory_filename(),
            memory_dirname: default_kernel_memory_memory_dirname(),
            workspace_memory_flush_enabled: true,
            workspace_memory_flush_on_compaction: true,
            flush_file_pattern: default_kernel_memory_flush_file_pattern(),
            tool_search_enabled: true,
            tool_get_enabled: true,
            search_max_results: default_kernel_memory_search_max_results(),
            search_min_score: default_kernel_memory_search_min_score(),
            search_include_root_memory: true,
            search_include_daily_memory: true,
            search_include_sessions: false,
        }
    }
}

/// Binding defaults under agent.kernel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelBindingConfig {
    /// Default kernel agent kind
    #[serde(default = "default_kernel_binding_agent_kind")]
    pub default_agent_kind: String,
}

fn default_kernel_binding_agent_kind() -> String {
    "llm".to_string()
}

impl Default for AgentKernelBindingConfig {
    fn default() -> Self {
        Self {
            default_agent_kind: default_kernel_binding_agent_kind(),
        }
    }
}

impl Default for AgentKernelConfig {
    fn default() -> Self {
        Self {
            max_iters: default_kernel_max_iters(),
            max_task_per_turn: default_kernel_max_task_per_turn(),
            tool_parallelism: default_kernel_tool_parallelism(),
            max_llm_calls: default_kernel_max_llm_calls(),
            max_tool_rounds: default_kernel_max_tool_rounds(),
            tool_result_max_bytes: default_kernel_tool_result_max_bytes(),
            context_compaction_max_bytes: default_kernel_context_compaction_max_bytes(),
            default_workspace_id: default_workspace_id(),
            planner_mode: default_kernel_planner_mode(),
            allow_request_planner_override: default_kernel_allow_request_planner_override(),
            execution_mode: default_kernel_execution_mode(),
            execution_children: default_kernel_execution_children(),
            execution_loop_iters: default_kernel_execution_loop_iters(),
            prompt: AgentKernelPromptConfig::default(),
            memory: AgentKernelMemoryConfig::default(),
            binding: AgentKernelBindingConfig::default(),
        }
    }
}

/// Gateway 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// 监听地址
    #[serde(default = "default_host")]
    pub host: String,

    /// 监听端口
    #[serde(default = "default_port")]
    pub port: u16,

    /// 认证 Token
    #[serde(default)]
    pub token: Option<String>,

    /// Enable gateway auth checks.
    #[serde(default)]
    pub auth: bool,

    /// JWT secret used by gateway auth middleware.
    #[serde(default)]
    pub jwt_secret: Option<String>,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    18789
}

fn default_true() -> bool {
    true
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            token: None,
            auth: false,
            jwt_secret: None,
        }
    }
}

/// Channel configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelsConfig {
    pub whatsapp: ChannelConfig,
    pub telegram: TelegramChannelsConfig,
    pub discord: ChannelConfig,
    pub feishu: ChannelConfig,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            whatsapp: ChannelConfig::default(),
            telegram: TelegramChannelsConfig::default(),
            discord: ChannelConfig::default(),
            feishu: ChannelConfig::default(),
        }
    }
}

/// Generic channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub allow_from: Vec<String>,
    /// DM policy: open/pairing/allowlist
    #[serde(default = "default_dm_policy")]
    pub dm_policy: String,
    /// DM scope: per_channel_peer_topic/per_channel_peer/main
    #[serde(default = "default_dm_scope")]
    pub dm_scope: String,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: None,
            allow_from: Vec::new(),
            dm_policy: default_dm_policy(),
            dm_scope: default_dm_scope(),
        }
    }
}

/// Telegram channels config (multi-account support)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramChannelsConfig {
    pub enabled: bool,
    pub token: Option<String>,
    pub allow_from: Vec<String>,
    /// DM policy: open/pairing/allowlist
    pub dm_policy: String,
    /// Group policy: open/mention/allowlist
    pub group_policy: String,
    pub group_allow_from: Vec<String>,
    /// DM scope: per_channel_peer_topic/per_channel_peer/main
    pub dm_scope: String,
    /// Mode: polling/webhook
    pub mode: String,
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
    pub accounts: Vec<TelegramAccountConfig>,
}

/// Telegram account config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramAccountConfig {
    pub id: String,
    pub enabled: bool,
    pub token: Option<String>,
    pub token_file: Option<String>,
    /// DM policy: open/pairing/allowlist
    pub dm_policy: String,
    /// Group policy: open/mention/allowlist
    pub group_policy: String,
    pub allow_from: Vec<String>,
    pub group_allow_from: Vec<String>,
    /// Mode: polling/webhook
    pub mode: String,
    pub webhook_url: Option<String>,
    pub groups: Vec<TelegramGroupConfig>,
}

/// Telegram group config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramGroupConfig {
    pub id: String,
    pub require_mention: bool,
    pub allow_from: Vec<String>,
    pub topics: Vec<TelegramTopicConfig>,
}

/// Telegram topic config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramTopicConfig {
    pub id: String,
    pub require_mention: bool,
    pub allow_from: Vec<String>,
}

fn default_dm_policy() -> String {
    "open".to_string()
}

fn default_group_policy() -> String {
    "mention".to_string()
}

fn default_dm_scope() -> String {
    "per_channel_peer_topic".to_string()
}

fn default_telegram_mode() -> String {
    "polling".to_string()
}

impl Default for TelegramChannelsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: None,
            allow_from: Vec::new(),
            dm_policy: default_dm_policy(),
            group_policy: default_group_policy(),
            group_allow_from: Vec::new(),
            dm_scope: default_dm_scope(),
            mode: default_telegram_mode(),
            webhook_url: None,
            webhook_secret: None,
            accounts: Vec::new(),
        }
    }
}

impl Default for TelegramAccountConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            token: None,
            token_file: None,
            dm_policy: default_dm_policy(),
            group_policy: default_group_policy(),
            allow_from: Vec::new(),
            group_allow_from: Vec::new(),
            mode: default_telegram_mode(),
            webhook_url: None,
            groups: Vec::new(),
        }
    }
}

impl Default for TelegramGroupConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            require_mention: true,
            allow_from: Vec::new(),
            topics: Vec::new(),
        }
    }
}

impl Default for TelegramTopicConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            require_mention: false,
            allow_from: Vec::new(),
        }
    }
}

/// LLM 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// 默认提供者
    #[serde(default = "default_provider")]
    pub provider: String,

    /// 默认模型
    #[serde(default = "default_model")]
    pub model: String,

    /// Anthropic 配置
    #[serde(default)]
    pub anthropic: ProviderConfig,

    /// OpenAI 配置
    #[serde(default)]
    pub openai: ProviderConfig,

    /// Bailian 配置
    #[serde(default)]
    pub bailian: ProviderConfig,

    /// Ollama 配置
    #[serde(default)]
    pub ollama: ProviderConfig,
}

fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_model() -> String {
    "claude-3-5-sonnet".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            anthropic: ProviderConfig::default(),
            openai: ProviderConfig::default(),
            bailian: ProviderConfig::default(),
            ollama: ProviderConfig::default(),
        }
    }
}

/// 提供者配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// API Key
    #[serde(default)]
    pub api_key: Option<String>,

    /// Base URL (用于自定义 endpoint)
    #[serde(default)]
    pub base_url: Option<String>,

    /// 是否启用
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Provider-specific model metadata
    #[serde(default)]
    pub models: Vec<ProviderModelConfig>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            enabled: true,
            models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderModelConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub context_window: u32,
    #[serde(default)]
    pub max_tokens: u32,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub input: Vec<ModelInputKind>,
    #[serde(default)]
    pub cost: ModelCostConfig,
    #[serde(default)]
    pub compat: ModelCompatConfig,
}

impl ProviderModelConfig {
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            self.id.as_str()
        } else {
            self.name.as_str()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelInputKind {
    Text,
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelCostConfig {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

impl Default for ModelCostConfig {
    fn default() -> Self {
        Self {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ModelCompatConfig {
    #[serde(default)]
    pub thinking_format: Option<ThinkingFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingFormat {
    OpenAi,
    Anthropic,
    Qwen,
}

impl Config {
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
    /// Lookup order:
    /// 1. `~/.talos/talos.yaml` (home directory)
    /// 2. `talos.yaml` (current working directory)
    pub fn load() -> TalosResult<Self> {
        Self::load_from_path(None)
    }

    /// Load configuration from a specific path.
    pub fn load_from_path(path: Option<&str>) -> TalosResult<Self> {
        use talos_error::TalosError;

        let config_path = match path {
            Some(path) => PathBuf::from(path),
            None => {
                let candidates = default_config_candidates();
                select_first_existing_path(&candidates).ok_or_else(|| {
                    TalosError::InvalidConfiguration(format!(
                        "no config file found, tried: {}",
                        format_path_list(&candidates)
                    ))
                })?
            }
        };

        let builder = config::Config::builder()
            .add_source(config::File::from(config_path).required(true))
            // Keep TALOS__ overrides for e2e/integration scenarios.
            .add_source(config::Environment::with_prefix("TALOS").separator("__"));

        // Build merged config values from file + TALOS__ environment overrides.
        let config = builder
            .build()
            .map_err(|e| TalosError::ConfigError(e.to_string()))?;

        // Strictly require explicit config keys instead of relying on fallback defaults.
        let raw_value: Value = config
            .try_deserialize()
            .map_err(|e| TalosError::ConfigError(e.to_string()))?;
        validate_required_paths(&raw_value)?;

        // Deserialize into typed config after strict key presence validation.
        let config: Config = serde_json::from_value(raw_value)
            .map_err(|e| TalosError::ConfigError(e.to_string()))?;

        Ok(config.resolve_env_placeholders()?)
    }

    /// 保存到文件
    pub fn save(&self, path: &str) -> TalosResult<()> {
        use talos_error::TalosError;

        let yaml = serde_yaml::to_string(self)
            .map_err(|e| TalosError::ConfigError(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(path, yaml)
            .map_err(|e| TalosError::ConfigError(format!("Failed to write config file: {}", e)))?;

        Ok(())
    }

    /// 获取配置文件路径
    pub fn default_path() -> PathBuf {
        PathBuf::from("talos.yaml")
    }

    /// 获取用户配置目录
    pub fn user_config_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|d| d.join(".talos"))
    }

    fn resolve_env_placeholders(self) -> TalosResult<Self> {
        use talos_error::TalosError;

        let mut value = serde_json::to_value(self)
            .map_err(|e| TalosError::ConfigError(format!("Failed to encode config: {}", e)))?;
        Self::resolve_value_placeholders(&mut value);
        serde_json::from_value(value)
            .map_err(|e| TalosError::ConfigError(format!("Failed to decode config: {}", e)))
    }

    fn resolve_value_placeholders(value: &mut Value) {
        match value {
            Value::String(text) => {
                *text = Self::expand_env_in_string(text);
            }
            Value::Array(items) => {
                for item in items {
                    Self::resolve_value_placeholders(item);
                }
            }
            Value::Object(map) => {
                for v in map.values_mut() {
                    Self::resolve_value_placeholders(v);
                }
            }
            _ => {}
        }
    }

    fn expand_env_in_string(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let mut cursor = 0usize;
        while let Some(start_rel) = input[cursor..].find("${") {
            let start = cursor + start_rel;
            result.push_str(&input[cursor..start]);
            let after_start = start + 2;
            if let Some(end_rel) = input[after_start..].find('}') {
                let end = after_start + end_rel;
                let var_name = &input[after_start..end];
                let replacement =
                    env::var(var_name).unwrap_or_else(|_| format!("${{{}}}", var_name));
                result.push_str(&replacement);
                cursor = end + 1;
            } else {
                result.push_str(&input[start..]);
                cursor = input.len();
            }
        }
        if cursor < input.len() {
            result.push_str(&input[cursor..]);
        }
        result
    }

    /// Validate runtime-critical configuration values.
    pub fn validate_runtime(&self) -> TalosResult<()> {
        use talos_error::TalosError;

        if self.agent.name.trim().is_empty() || is_unresolved_placeholder(&self.agent.name) {
            return Err(TalosError::InvalidConfiguration(
                "agent.name is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.agent.model.trim().is_empty() || is_unresolved_placeholder(&self.agent.model) {
            return Err(TalosError::InvalidConfiguration(
                "agent.model is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.agent.kernel.max_iters == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.max_iters must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.max_task_per_turn == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.max_task_per_turn must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.tool_parallelism == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.tool_parallelism must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.max_llm_calls == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.max_llm_calls must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.max_tool_rounds == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.max_tool_rounds must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.tool_result_max_bytes == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.tool_result_max_bytes must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.context_compaction_max_bytes == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.context_compaction_max_bytes must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.default_workspace_id.trim().is_empty()
            || is_unresolved_placeholder(&self.agent.kernel.default_workspace_id)
        {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.default_workspace_id is empty or unresolved placeholder".to_string(),
            ));
        }
        let planner_mode = self.agent.kernel.planner_mode.trim().to_ascii_lowercase();
        if planner_mode != "built_in" && planner_mode != "plan_react" {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.planner_mode must be one of: built_in, plan_react".to_string(),
            ));
        }
        let execution_mode = self.agent.kernel.execution_mode.trim().to_ascii_lowercase();
        if execution_mode != "single"
            && execution_mode != "sequential"
            && execution_mode != "parallel"
            && execution_mode != "loop"
            && execution_mode != "a2a"
        {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.execution_mode must be one of: single, sequential, parallel, loop, a2a".to_string(),
            ));
        }
        if self.agent.kernel.execution_children == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.execution_children must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.execution_loop_iters == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.execution_loop_iters must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.prompt.max_static_tokens == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.prompt.max_static_tokens must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.prompt.max_dynamic_tokens == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.prompt.max_dynamic_tokens must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.memory.workspace_dir.trim().is_empty()
            || is_unresolved_placeholder(&self.agent.kernel.memory.workspace_dir)
        {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.workspace_dir is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.agent.kernel.memory.hot_recent_messages == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.hot_recent_messages must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.memory.session_recent_messages_limit == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.session_recent_messages_limit must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.memory.session_summary_recent_count == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.session_summary_recent_count must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.memory.session_summary_max_chars == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.session_summary_max_chars must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.memory.compaction_threshold_bytes == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.compaction_threshold_bytes must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.memory.search_min_score < 0.0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.search_min_score must be >= 0.0".to_string(),
            ));
        }
        if self
            .agent
            .kernel
            .memory
            .root_memory_filename
            .trim()
            != "MEMORY.md"
        {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.memory.root_memory_filename must be MEMORY.md".to_string(),
            ));
        }
        if self.agent.kernel.prompt.bootstrap.max_chars_per_file == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.prompt.bootstrap.max_chars_per_file must be > 0".to_string(),
            ));
        }
        if self.agent.kernel.prompt.bootstrap.max_chars_total == 0 {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.prompt.bootstrap.max_chars_total must be > 0".to_string(),
            ));
        }
        if self
            .agent
            .kernel
            .binding
            .default_agent_kind
            .trim()
            .is_empty()
            || is_unresolved_placeholder(&self.agent.kernel.binding.default_agent_kind)
        {
            return Err(TalosError::InvalidConfiguration(
                "agent.kernel.binding.default_agent_kind is empty or unresolved placeholder"
                    .to_string(),
            ));
        }

        if self.gateway.host.trim().is_empty() || is_unresolved_placeholder(&self.gateway.host) {
            return Err(TalosError::InvalidConfiguration(
                "gateway.host is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.gateway.port == 0 {
            return Err(TalosError::InvalidConfiguration(
                "gateway.port must be > 0".to_string(),
            ));
        }
        let Some(token) = self.gateway.token.as_ref() else {
            return Err(TalosError::InvalidConfiguration(
                "gateway.token is required".to_string(),
            ));
        };
        if token.trim().is_empty() || is_unresolved_placeholder(token) {
            return Err(TalosError::InvalidConfiguration(
                "gateway.token is empty or unresolved placeholder".to_string(),
            ));
        }

        if self.gateway.auth {
            let Some(secret) = self.gateway.jwt_secret.as_ref() else {
                return Err(TalosError::InvalidConfiguration(
                    "gateway.jwt_secret is required when gateway.auth=true".to_string(),
                ));
            };
            if secret.trim().is_empty() || is_unresolved_placeholder(secret) {
                return Err(TalosError::InvalidConfiguration(
                    "gateway.jwt_secret is empty or unresolved placeholder".to_string(),
                ));
            }
        }

        if self.logging.level.trim().is_empty() || is_unresolved_placeholder(&self.logging.level) {
            return Err(TalosError::InvalidConfiguration(
                "logging.level is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.logging.format.trim().is_empty() || is_unresolved_placeholder(&self.logging.format)
        {
            return Err(TalosError::InvalidConfiguration(
                "logging.format is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.logging.format != "text" && self.logging.format != "json" {
            return Err(TalosError::InvalidConfiguration(
                "logging.format must be one of: text, json".to_string(),
            ));
        }

        if self.storage.database_type.trim().is_empty()
            || is_unresolved_placeholder(&self.storage.database_type)
        {
            return Err(TalosError::InvalidConfiguration(
                "storage.database_type is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.storage.database_type != "sqlite" {
            return Err(TalosError::InvalidConfiguration(format!(
                "unsupported storage.database_type: {}",
                self.storage.database_type
            )));
        }
        if self.storage.connection_string.trim().is_empty()
            || is_unresolved_placeholder(&self.storage.connection_string)
        {
            return Err(TalosError::InvalidConfiguration(
                "storage.connection_string is empty or unresolved placeholder".to_string(),
            ));
        }
        if self.storage.max_connections == 0 {
            return Err(TalosError::InvalidConfiguration(
                "storage.max_connections must be > 0".to_string(),
            ));
        }

        let provider_name = self.llm.provider.trim().to_ascii_lowercase();
        let provider = match provider_name.as_str() {
            "anthropic" => ("anthropic", &self.llm.anthropic),
            "openai" => ("openai", &self.llm.openai),
            "bailian" => ("bailian", &self.llm.bailian),
            "ollama" => ("ollama", &self.llm.ollama),
            other => {
                return Err(TalosError::InvalidConfiguration(format!(
                    "unsupported llm.provider: {}",
                    other
                )));
            }
        };

        if !provider.1.enabled {
            return Err(TalosError::InvalidConfiguration(format!(
                "llm provider '{}' is disabled",
                provider.0
            )));
        }

        if provider.0 != "ollama" {
            let Some(api_key) = provider.1.api_key.as_ref() else {
                return Err(TalosError::InvalidConfiguration(format!(
                    "llm.{}.api_key is required",
                    provider.0
                )));
            };
            if api_key.trim().is_empty() || is_unresolved_placeholder(api_key) {
                return Err(TalosError::InvalidConfiguration(format!(
                    "llm.{}.api_key is empty or unresolved placeholder",
                    provider.0
                )));
            }
        }

        if let Some(base_url) = provider.1.base_url.as_ref() {
            if base_url.trim().is_empty() || is_unresolved_placeholder(base_url) {
                return Err(TalosError::InvalidConfiguration(format!(
                    "llm.{}.base_url is empty or unresolved placeholder",
                    provider.0
                )));
            }
        } else if provider.0 == "bailian" || provider.0 == "ollama" {
            return Err(TalosError::InvalidConfiguration(format!(
                "llm.{}.base_url is required",
                provider.0
            )));
        }

        if self.llm.model.trim().is_empty() || is_unresolved_placeholder(&self.llm.model) {
            return Err(TalosError::InvalidConfiguration(
                "llm.model is empty or unresolved placeholder".to_string(),
            ));
        }

        if !provider.1.models.is_empty()
            && !provider
                .1
                .models
                .iter()
                .any(|model| model.id == self.llm.model)
        {
            return Err(TalosError::InvalidConfiguration(format!(
                "llm.model '{}' is not configured under llm.{}.models",
                self.llm.model, provider.0
            )));
        }

        Ok(())
    }
}

const REQUIRED_CONFIG_PATHS: &[&str] = &[
    "agent.name",
    "agent.model",
    "agent.kernel.max_iters",
    "agent.kernel.max_task_per_turn",
    "agent.kernel.tool_parallelism",
    "agent.kernel.max_llm_calls",
    "agent.kernel.max_tool_rounds",
    "agent.kernel.tool_result_max_bytes",
    "agent.kernel.context_compaction_max_bytes",
    "agent.kernel.default_workspace_id",
    "agent.kernel.planner_mode",
    "agent.kernel.allow_request_planner_override",
    "agent.kernel.execution_mode",
    "agent.kernel.execution_children",
    "agent.kernel.execution_loop_iters",
    "agent.kernel.prompt.max_static_tokens",
    "agent.kernel.prompt.max_dynamic_tokens",
    "agent.kernel.prompt.include_session_summary",
    "agent.kernel.prompt.include_recent_messages",
    "agent.kernel.binding.default_agent_kind",
    "gateway.host",
    "gateway.port",
    "gateway.token",
    "gateway.auth",
    "gateway.jwt_secret",
    "logging.level",
    "logging.format",
    "logging.stdout",
    "storage.database_type",
    "storage.connection_string",
    "storage.max_connections",
    "llm.provider",
    "llm.model",
    "llm.anthropic.enabled",
    "llm.openai.enabled",
    "llm.bailian.enabled",
    "llm.ollama.enabled",
];

fn validate_required_paths(root: &Value) -> TalosResult<()> {
    use talos_error::TalosError;

    let missing: Vec<&str> = REQUIRED_CONFIG_PATHS
        .iter()
        .copied()
        .filter(|path| !has_json_path(root, path))
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    Err(TalosError::InvalidConfiguration(format!(
        "missing required config key(s): {}",
        missing.join(", ")
    )))
}

fn has_json_path(root: &Value, path: &str) -> bool {
    let mut node = root;
    for seg in path.split('.') {
        match node {
            Value::Object(map) => {
                let Some(next) = map.get(seg) else {
                    return false;
                };
                node = next;
            }
            _ => return false,
        }
    }
    true
}

fn is_unresolved_placeholder(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("${") && trimmed.ends_with('}')
}

fn default_config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(2);
    if let Some(home_dir) = dirs::home_dir() {
        candidates.push(home_dir.join(".talos").join("talos.yaml"));
    }
    candidates.push(Config::default_path());
    candidates
}

fn select_first_existing_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| path.exists()).cloned()
}

fn format_path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// 生成默认配置文件
pub fn generate_default_config() -> String {
    r#"# Talos Configuration
# https://github.com/xiliu/talos

# Agent 配置
agent:
  name: jarvis
  model: claude-3-5-sonnet
  kernel:
    max_iters: 1
    max_task_per_turn: 3
    tool_parallelism: 4
    max_llm_calls: 8
    max_tool_rounds: 8
    tool_result_max_bytes: 65536
    context_compaction_max_bytes: 262144
    default_workspace_id: default
    planner_mode: built_in
    allow_request_planner_override: false
    execution_mode: single
    execution_children: 2
    execution_loop_iters: 3
    prompt:
      max_static_tokens: 1200
      max_dynamic_tokens: 3200
      include_session_summary: true
      include_recent_messages: true
    binding:
      default_agent_kind: llm

# Gateway 配置
gateway:
  host: 127.0.0.1
  port: 18789
  token: ${GATEWAY_TOKEN}  # 环境变量
  jwt_secret: ${JWT_SECRET}
  auth: false

# 渠道配置
channels:
  whatsapp:
    enabled: false
    dm_policy: pairing
    allow_from: []
  
  telegram:
    enabled: false
    token: ${TELEGRAM_BOT_TOKEN}
    dm_policy: pairing
    dm_scope: per_channel_peer_topic
  
  discord:
    enabled: false
    token: ${DISCORD_BOT_TOKEN}
  
  feishu:
    enabled: false
    token: ${FEISHU_APP_ID}:${FEISHU_APP_SECRET}

# LLM 配置
llm:
  provider: anthropic
  model: claude-3-5-sonnet
  
  anthropic:
    api_key: ${ANTHROPIC_API_KEY}
    enabled: true
  
  openai:
    api_key: ${OPENAI_API_KEY}
    enabled: true

  bailian:
    api_key: ${BAILIAN_API_KEY}
    base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
    enabled: true
    models:
      - id: qwen-max
        name: qwen-max
        context_window: 32768
        max_tokens: 8192
        reasoning: false
        input: [text]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
        compat: {}
      - id: qwen-plus
        name: qwen-plus
        context_window: 32768
        max_tokens: 8192
        reasoning: false
        input: [text]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
        compat: {}
      - id: qwen-turbo
        name: qwen-turbo
        context_window: 32768
        max_tokens: 8192
        reasoning: false
        input: [text]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
        compat: {}
  
  ollama:
    base_url: http://localhost:11434
    enabled: true

# 日志配置
logging:
  level: info
  format: text  # json or text
  stdout: true
  file: logs/talos.log

# Storage 配置
storage:
  database_type: sqlite
  connection_string: talos.db
  max_connections: 5
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.agent.name, "talos");
        assert_eq!(config.gateway.port, 18789);
        assert!(!config.gateway.auth);
        assert_eq!(config.storage.database_type, "sqlite");
        assert_eq!(config.storage.connection_string, "talos.db");
    }

    #[test]
    fn test_generate_default() {
        let yaml = generate_default_config();
        assert!(yaml.contains("agent:"));
        assert!(yaml.contains("gateway:"));
    }

    #[test]
    fn test_kernel_defaults() {
        let config = Config::default();
        assert_eq!(config.agent.kernel.max_iters, 1);
        assert_eq!(config.agent.kernel.max_task_per_turn, 3);
        assert_eq!(config.agent.kernel.max_tool_rounds, 8);
        assert_eq!(config.agent.kernel.tool_result_max_bytes, 64 * 1024);
        assert_eq!(config.agent.kernel.context_compaction_max_bytes, 256 * 1024);
        assert_eq!(config.agent.kernel.default_workspace_id, "default");
        assert_eq!(config.agent.kernel.planner_mode, "built_in");
        assert!(!config.agent.kernel.allow_request_planner_override);
        assert_eq!(config.agent.kernel.execution_mode, "single");
        assert_eq!(config.agent.kernel.execution_children, 2);
        assert_eq!(config.agent.kernel.execution_loop_iters, 3);
        assert_eq!(config.agent.kernel.prompt.max_static_tokens, 1200);
        assert_eq!(config.agent.kernel.prompt.max_dynamic_tokens, 3200);
        assert!(config.agent.kernel.prompt.include_session_summary);
        assert!(config.agent.kernel.prompt.include_recent_messages);
        assert!(config.agent.kernel.prompt.include_root_memory);
        assert!(config.agent.kernel.prompt.include_retrieved_memories);
        assert!(config.agent.kernel.prompt.bootstrap.enabled);
        assert!(config.agent.kernel.prompt.bootstrap.auto_create_missing_files);
        assert_eq!(config.agent.kernel.prompt.bootstrap.max_chars_per_file, 8_000);
        assert_eq!(config.agent.kernel.prompt.bootstrap.max_chars_total, 24_000);
        assert_eq!(
            config.agent.kernel.memory.workspace_dir,
            "~/.talos/workspace"
        );
        assert_eq!(config.agent.kernel.memory.hot_recent_messages, 64);
        assert_eq!(config.agent.kernel.memory.session_recent_messages_limit, 24);
        assert!(config.agent.kernel.memory.session_summary_enabled);
        assert_eq!(config.agent.kernel.memory.session_summary_recent_count, 2);
        assert_eq!(config.agent.kernel.memory.session_summary_max_chars, 1200);
        assert_eq!(
            config.agent.kernel.memory.compaction_threshold_bytes,
            256 * 1024
        );
        assert_eq!(config.agent.kernel.memory.root_memory_filename, "MEMORY.md");
        assert!(config.agent.kernel.memory.workspace_memory_flush_enabled);
        assert!((config.agent.kernel.memory.search_min_score - 0.35).abs() < f32::EPSILON);
        assert_eq!(config.agent.kernel.binding.default_agent_kind, "llm");
    }

    #[test]
    fn test_expand_env_in_string() {
        let raw = "token=${A}:${B}";
        // Keep missing env placeholders as-is so behavior is predictable.
        let expanded = Config::expand_env_in_string(raw);
        assert!(expanded.contains("${A}") || expanded.contains(':'));
    }

    #[test]
    fn test_validate_runtime_requires_jwt_secret_when_auth_enabled() {
        let mut config = Config::default();
        config.gateway.auth = true;
        config.gateway.jwt_secret = None;
        config.gateway.token = Some("token".to_string());
        assert!(config.validate_runtime().is_err());

        config.gateway.jwt_secret = Some("jwt-secret".to_string());
        config.llm.provider = "ollama".to_string();
        config.llm.model = "qwen-max".to_string();
        config.llm.ollama.base_url = Some("http://localhost:11434".to_string());
        config.agent.model = "qwen-max".to_string();
        assert!(config.validate_runtime().is_ok());
    }

    #[test]
    fn test_validate_runtime_requires_provider_api_key() {
        let mut config = Config::default();
        config.gateway.auth = false;
        config.gateway.token = Some("token".to_string());
        config.llm.provider = "openai".to_string();
        config.llm.model = "gpt-4o".to_string();
        config.agent.model = "gpt-4o".to_string();
        config.llm.openai.base_url = Some("https://api.openai.com/v1".to_string());
        config.llm.openai.api_key = None;
        assert!(config.validate_runtime().is_err());

        config.llm.openai.api_key = Some("sk-test".to_string());
        assert!(config.validate_runtime().is_ok());
    }

    #[test]
    fn test_provider_models_are_strongly_typed() {
        let yaml = r#"
enabled: true
api_key: sk-test
base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
models:
  - id: qwen-max
    name: qwen-max
    context_window: 32768
    max_tokens: 8192
    reasoning: false
    input: [text, image]
    cost:
      input: 0.1
      output: 0.2
      cache_read: 0.01
      cache_write: 0.02
    compat:
      thinking_format: qwen
"#;

        let provider: ProviderConfig =
            serde_yaml::from_str(yaml).expect("provider config should parse");
        let model = &provider.models[0];
        assert_eq!(model.id, "qwen-max");
        assert_eq!(model.context_window, 32_768);
        assert_eq!(
            model.input,
            vec![ModelInputKind::Text, ModelInputKind::Image]
        );
        assert_eq!(model.compat.thinking_format, Some(ThinkingFormat::Qwen));
        assert!(provider.enabled);
    }

    #[test]
    fn test_load_from_path_fails_when_required_keys_missing() {
        let temp_file =
            std::env::temp_dir().join(format!("talos-config-missing-{}.yaml", std::process::id()));
        std::fs::write(&temp_file, "gateway:\n  host: 127.0.0.1\n")
            .expect("failed to write temp config");
        let result = Config::load_from_path(Some(
            temp_file
                .to_str()
                .expect("temp path should be valid unicode"),
        ));
        let _ = std::fs::remove_file(&temp_file);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_path_fails_when_file_missing() {
        let missing_file = std::env::temp_dir().join(format!(
            "talos-config-not-found-{}.yaml",
            std::process::id()
        ));
        let result = Config::load_from_path(Some(
            missing_file
                .to_str()
                .expect("temp path should be valid unicode"),
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_select_first_existing_path_prefers_first_match() {
        let base_dir =
            std::env::temp_dir().join(format!("talos-config-select-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base_dir);
        let first = base_dir.join("first.yaml");
        let second = base_dir.join("second.yaml");
        std::fs::write(&first, "x: 1\n").expect("failed to write first file");
        std::fs::write(&second, "x: 2\n").expect("failed to write second file");

        let selected = select_first_existing_path(&[first.clone(), second.clone()]);
        assert_eq!(selected, Some(first));

        let _ = std::fs::remove_file(second);
        let _ = std::fs::remove_file(base_dir.join("first.yaml"));
        let _ = std::fs::remove_dir_all(base_dir);
    }
}
