use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    // User PAT support is part of auth middleware path and is enabled together with `auth`.
    // There is no separate on/off toggle currently for PATs.
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    18789
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            auth: false,
        }
    }
}

/// Channel configurations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelsConfig {
    pub whatsapp: ChannelConfig,
    pub telegram: TelegramChannelsConfig,
    pub discord: ChannelConfig,
    pub feishu: ChannelConfig,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

    /// DeepSeek config
    #[serde(default)]
    pub deepseek: ProviderConfig,

    /// Ollama config
    #[serde(default)]
    pub ollama: ProviderConfig,

    /// OpenRouter config
    #[serde(default)]
    pub openrouter: ProviderConfig,
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
            deepseek: ProviderConfig::default(),
            ollama: ProviderConfig::default(),
            openrouter: ProviderConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProtocol {
    Chat,
    Responses,
    Messages,
}

impl LlmProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Responses => "responses",
            Self::Messages => "messages",
        }
    }
}

impl std::fmt::Display for LlmProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const CHAT_ONLY_PROTOCOLS: &[LlmProtocol] = &[LlmProtocol::Chat];
const OPENAI_PROTOCOLS: &[LlmProtocol] = &[LlmProtocol::Chat, LlmProtocol::Responses];
const MESSAGES_ONLY_PROTOCOLS: &[LlmProtocol] = &[LlmProtocol::Messages];
const DEEPSEEK_PROTOCOLS: &[LlmProtocol] = &[LlmProtocol::Chat, LlmProtocol::Messages];
const OPENROUTER_PROTOCOLS: &[LlmProtocol] = &[
    LlmProtocol::Chat,
    LlmProtocol::Responses,
    LlmProtocol::Messages,
];

const PROVIDER_PROTOCOLS: &[(&str, LlmProtocol, &[LlmProtocol])] = &[
    ("anthropic", LlmProtocol::Messages, MESSAGES_ONLY_PROTOCOLS),
    ("openai", LlmProtocol::Chat, OPENAI_PROTOCOLS),
    ("bailian", LlmProtocol::Chat, CHAT_ONLY_PROTOCOLS),
    ("deepseek", LlmProtocol::Chat, DEEPSEEK_PROTOCOLS),
    ("ollama", LlmProtocol::Chat, CHAT_ONLY_PROTOCOLS),
    ("openrouter", LlmProtocol::Chat, OPENROUTER_PROTOCOLS),
];

/// Protocol capabilities exposed to configuration clients for one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProtocolCapabilities {
    pub default_protocol: LlmProtocol,
    pub protocols: Vec<LlmProtocol>,
}

fn provider_protocols(
    provider: &str,
) -> Option<&'static (&'static str, LlmProtocol, &'static [LlmProtocol])> {
    let provider = provider.trim();
    PROVIDER_PROTOCOLS
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(provider))
}

pub fn default_llm_protocol(provider: &str) -> Option<LlmProtocol> {
    provider_protocols(provider).map(|(_, default_protocol, _)| *default_protocol)
}

pub fn allowed_llm_protocols(provider: &str) -> Option<&'static [LlmProtocol]> {
    provider_protocols(provider).map(|(_, _, protocols)| *protocols)
}

/// Returns the complete provider protocol matrix from the validation source of truth.
pub fn provider_protocol_capabilities() -> BTreeMap<String, ProviderProtocolCapabilities> {
    PROVIDER_PROTOCOLS
        .iter()
        .map(|(provider, default_protocol, protocols)| {
            (
                (*provider).to_string(),
                ProviderProtocolCapabilities {
                    default_protocol: *default_protocol,
                    protocols: protocols.to_vec(),
                },
            )
        })
        .collect()
}

pub fn effective_llm_protocol(provider: &str, config: &ProviderConfig) -> Option<LlmProtocol> {
    config.protocol.or_else(|| default_llm_protocol(provider))
}

pub fn llm_provider_supports_protocol(provider: &str, protocol: LlmProtocol) -> bool {
    allowed_llm_protocols(provider).is_some_and(|allowed| allowed.contains(&protocol))
}

/// Provider config
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// API key
    #[serde(default)]
    pub api_key: Option<String>,

    /// Base URL for custom endpoint
    #[serde(default)]
    pub base_url: Option<String>,

    /// Wire protocol used for this provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<LlmProtocol>,

    /// Provider-specific model metadata
    #[serde(default)]
    pub models: Vec<ProviderModelConfig>,
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
    pub reasoning_effort: ReasoningEffort,
    #[serde(default)]
    pub speed: ModelSpeed,
    #[serde(default)]
    pub input: Vec<ModelInputKind>,
    #[serde(default)]
    pub cost: ModelCostConfig,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    #[default]
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelSpeed {
    #[default]
    Standard,
    Fast,
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
