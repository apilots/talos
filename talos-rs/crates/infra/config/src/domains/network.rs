use serde::{Deserialize, Serialize};

/// Gateway configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    /// Listen host
    #[serde(default = "default_host")]
    pub host: String,

    /// Listen port
    #[serde(default = "default_port")]
    pub port: u16,

    /// Enable gateway auth checks.
    #[serde(default)]
    pub auth: bool,

    /// User PAT support is part of auth middleware path and is enabled together with `auth`.
    /// There is no separate on/off toggle currently for PATs.

    /// Authentication cache settings for cloud token introspection.
    #[serde(default)]
    pub auth_cache: GatewayAuthCacheConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayAuthCacheConfig {
    /// Enable auth-related in-memory caches.
    #[serde(default = "default_auth_cache_enabled")]
    pub enabled: bool,

    /// Cache TTL in seconds.
    #[serde(default = "default_auth_cache_ttl_seconds")]
    pub ttl_seconds: u64,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    18789
}

fn default_auth_cache_enabled() -> bool {
    true
}

fn default_auth_cache_ttl_seconds() -> u64 {
    300
}

impl Default for GatewayAuthCacheConfig {
    fn default() -> Self {
        Self {
            enabled: default_auth_cache_enabled(),
            ttl_seconds: default_auth_cache_ttl_seconds(),
        }
    }
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            auth: false,
            auth_cache: GatewayAuthCacheConfig::default(),
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
    pub dm_policy: DmPolicy,
    /// DM scope: per_channel_peer_topic/per_channel_peer/main
    #[serde(default = "default_dm_scope")]
    pub dm_scope: DmScope,
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
    pub dm_policy: DmPolicy,
    /// Group policy: open/mention/allowlist
    pub group_policy: GroupPolicy,
    pub group_allow_from: Vec<String>,
    /// DM scope: per_channel_peer_topic/per_channel_peer/main
    pub dm_scope: DmScope,
    /// Mode: polling/webhook
    pub mode: TelegramMode,
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
    pub dm_policy: DmPolicy,
    /// Group policy: open/mention/allowlist
    pub group_policy: GroupPolicy,
    pub allow_from: Vec<String>,
    pub group_allow_from: Vec<String>,
    /// Mode: polling/webhook
    pub mode: TelegramMode,
    pub webhook_url: Option<String>,
    pub groups: Vec<TelegramGroupConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DmPolicy {
    #[default]
    Open,
    Pairing,
    Allowlist,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GroupPolicy {
    Open,
    #[default]
    Mention,
    Allowlist,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DmScope {
    #[default]
    PerChannelPeerTopic,
    PerChannelPeer,
    Main,
}

impl DmScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PerChannelPeerTopic => "per_channel_peer_topic",
            Self::PerChannelPeer => "per_channel_peer",
            Self::Main => "main",
        }
    }
}

impl std::fmt::Display for DmScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TelegramMode {
    #[default]
    Polling,
    Webhook,
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

fn default_dm_policy() -> DmPolicy {
    DmPolicy::Open
}

fn default_group_policy() -> GroupPolicy {
    GroupPolicy::Mention
}

fn default_dm_scope() -> DmScope {
    DmScope::PerChannelPeerTopic
}

fn default_telegram_mode() -> TelegramMode {
    TelegramMode::Polling
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

/// LLM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Default provider
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Default model
    #[serde(default = "default_model")]
    pub model: String,

    /// Anthropic config
    #[serde(default)]
    pub anthropic: ProviderConfig,

    /// OpenAI config
    #[serde(default)]
    pub openai: ProviderConfig,

    /// Bailian config
    #[serde(default)]
    pub bailian: ProviderConfig,

    /// Ollama config
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

/// Provider config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// API key
    #[serde(default)]
    pub api_key: Option<String>,

    /// Base URL for custom endpoint
    #[serde(default)]
    pub base_url: Option<String>,

    /// Enabled flag
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
#[serde(deny_unknown_fields)]
pub struct ProviderModelConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub context_window: u32,
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

fn default_true() -> bool {
    true
}
