use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use talos_config::{
    DmPolicy as ConfigDmPolicy, GroupPolicy as ConfigGroupPolicy,
    TelegramAccountConfig as ConfigAccountConfig, TelegramMode as ConfigTelegramMode,
};
use talos_error::{TalosError, TalosResult};
use tokio::sync::RwLock;
use tracing::info;

use crate::traits::{
    Channel, ChannelAccountSnapshot, ChannelCapabilities, ChannelDefinition, ChannelRuntimeContext,
};
use crate::types::{DmPolicy, GroupPolicy, MediaType};

use super::handler::TelegramMessageHandler;
use super::outbound::TelegramOutboundAdapter;
use super::streaming::TelegramStreamingAdapter;
use super::webhook::{TelegramWebhookHandler, WebhookManager};

pub struct TelegramChannel {
    definition: ChannelDefinition,
    bots: Arc<RwLock<HashMap<String, TelegramBotInstance>>>,
    states: Arc<RwLock<HashMap<String, ChannelAccountSnapshot>>>,
    outbound: TelegramOutboundAdapter,
    streaming: TelegramStreamingAdapter,
    webhook_manager: Arc<RwLock<WebhookManager>>,
}

impl TelegramChannel {
    pub fn new() -> Self {
        Self {
            definition: ChannelDefinition {
                id: "telegram",
                name: "Telegram",
                capabilities: ChannelCapabilities {
                    supports_threads: true,
                    supports_edit: true,
                    supports_typing: true,
                    supports_media: vec![
                        MediaType::Photo,
                        MediaType::Document,
                        MediaType::Voice,
                        MediaType::Video,
                    ],
                },
            },
            bots: Arc::new(RwLock::new(HashMap::new())),
            states: Arc::new(RwLock::new(HashMap::new())),
            outbound: TelegramOutboundAdapter::new(),
            streaming: TelegramStreamingAdapter::new(),
            webhook_manager: Arc::new(RwLock::new(WebhookManager::new())),
        }
    }

    pub fn get_enabled_accounts(
        &self,
        config: &talos_config::TelegramChannelsConfig,
    ) -> Vec<AccountConfig> {
        let mut accounts = Vec::new();

        if !config.accounts.is_empty() {
            for acc in &config.accounts {
                if acc.enabled {
                    accounts.push(AccountConfig::from_config(acc));
                }
            }
        } else if config.enabled {
            if let Some(token) = &config.token {
                accounts.push(AccountConfig {
                    id: "default".to_string(),
                    token: token.clone(),
                    token_file: None,
                    dm_policy: map_dm_policy(config.dm_policy),
                    group_policy: map_group_policy(config.group_policy),
                    allow_from: config.allow_from.clone(),
                    group_allow_from: config.group_allow_from.clone(),
                    mode: map_mode(config.mode),
                    webhook_url: config.webhook_url.clone(),
                    groups: vec![],
                });
            }
        }

        accounts
    }

    fn create_context_with_state(
        &self,
        account_id: String,
        ctx: ChannelRuntimeContext,
    ) -> ChannelRuntimeContext {
        let states = self.states.clone();
        let states2 = self.states.clone();
        let account_id_clone = account_id.clone();
        let account_id_clone2 = account_id.clone();
        ChannelRuntimeContext {
            config: ctx.config,
            account_id,
            abort_signal: ctx.abort_signal,
            event_tx: ctx.event_tx,
            get_status: Arc::new(move || {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        states
                            .read()
                            .await
                            .get(&account_id_clone)
                            .cloned()
                            .unwrap_or_default()
                    })
                })
            }),
            set_status: Arc::new(move |snapshot| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        states2
                            .write()
                            .await
                            .insert(account_id_clone2.clone(), snapshot);
                    })
                })
            }),
        }
    }
}

impl Default for TelegramChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn definition(&self) -> &ChannelDefinition {
        &self.definition
    }

    fn enabled_account_ids(&self, config: &talos_config::Config) -> Vec<String> {
        self.get_enabled_accounts(&config.channels.telegram)
            .into_iter()
            .map(|account| account.id)
            .collect()
    }

    async fn start_account(&self, ctx: ChannelRuntimeContext) -> TalosResult<()> {
        let telegram_config = &ctx.config.channels.telegram;
        let account = self
            .get_enabled_accounts(telegram_config)
            .into_iter()
            .find(|account| account.id == ctx.account_id)
            .ok_or_else(|| {
                TalosError::ConfigError(format!(
                    "Telegram account '{}' is not enabled or not configured",
                    ctx.account_id
                ))
            })?;

        let account_id = account.id.clone();
        if self.bots.read().await.contains_key(&account_id) {
            info!("Telegram account {} already started; skipping", account_id);
            return Ok(());
        }

        let mode = account.mode;
        let bot_token = account.token.clone();
        info!(
            "Starting Telegram account: {} (mode: {:?})",
            account_id, mode
        );

        let ctx_with_state = self.create_context_with_state(account_id.clone(), ctx.clone());
        let bot = TelegramBotInstance::new(account, ctx_with_state.clone()).await?;

        if let Some(ref handler) = bot.webhook_handler {
            self.webhook_manager
                .write()
                .await
                .register_handler(handler.clone());
        }

        (ctx_with_state.set_status)(ChannelAccountSnapshot {
            account_id: account_id.clone(),
            enabled: true,
            configured: true,
            running: true,
            last_start_at: Some(chrono::Utc::now().timestamp()),
            mode: Some(format!("{:?}", mode)),
            ..Default::default()
        });

        self.bots.write().await.insert(account_id.clone(), bot);

        self.outbound.register_bot(account_id, bot_token).await;

        Ok(())
    }

    async fn stop_account(&self, account_id: &str) -> TalosResult<()> {
        info!("Stopping Telegram account: {}", account_id);

        if let Some(bot) = self.bots.write().await.remove(account_id) {
            bot.abort_signal.cancel();
        }

        if let Some(state) = self.states.write().await.get_mut(account_id) {
            state.running = false;
            state.last_stop_at = Some(chrono::Utc::now().timestamp());
        }

        Ok(())
    }

    fn outbound_adapter(&self) -> Option<&dyn crate::traits::OutboundAdapter> {
        Some(&self.outbound)
    }

    fn streaming_adapter(&self) -> Option<&dyn crate::traits::StreamingAdapter> {
        Some(&self.streaming)
    }
}

pub struct TelegramBotInstance {
    #[allow(dead_code)]
    pub account_id: String,
    pub abort_signal: tokio_util::sync::CancellationToken,
    pub webhook_handler: Option<TelegramWebhookHandler>,
}

impl TelegramBotInstance {
    pub async fn new(config: AccountConfig, ctx: ChannelRuntimeContext) -> TalosResult<Self> {
        let abort_signal = ctx.abort_signal.clone();
        let account_id = config.id.clone();
        let mode = config.mode;

        let webhook_handler = match mode {
            Mode::Polling => {
                let handler = TelegramMessageHandler::new(config, ctx);
                tokio::spawn(async move {
                    handler.run().await;
                });
                None
            }
            Mode::Webhook => {
                let handler = TelegramWebhookHandler::new(config, ctx);
                Some(handler)
            }
        };

        Ok(Self {
            account_id,
            abort_signal,
            webhook_handler,
        })
    }
}

#[derive(Clone)]
pub struct AccountConfig {
    pub id: String,
    pub token: String,
    pub token_file: Option<String>,
    pub dm_policy: DmPolicy,
    pub group_policy: GroupPolicy,
    pub allow_from: Vec<String>,
    pub group_allow_from: Vec<String>,
    pub mode: Mode,
    pub webhook_url: Option<String>,
    pub groups: Vec<GroupConfig>,
}

impl AccountConfig {
    fn from_config(cfg: &ConfigAccountConfig) -> Self {
        Self {
            id: cfg.id.clone(),
            token: cfg.token.clone().unwrap_or_default(),
            token_file: cfg.token_file.clone(),
            dm_policy: map_dm_policy(cfg.dm_policy),
            group_policy: map_group_policy(cfg.group_policy),
            allow_from: cfg.allow_from.clone(),
            group_allow_from: cfg.group_allow_from.clone(),
            mode: map_mode(cfg.mode),
            webhook_url: cfg.webhook_url.clone(),
            groups: cfg.groups.iter().map(GroupConfig::from_config).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GroupConfig {
    pub id: String,
    pub require_mention: bool,
    pub allow_from: Vec<String>,
    pub topics: Vec<TopicConfig>,
}

impl GroupConfig {
    fn from_config(cfg: &talos_config::TelegramGroupConfig) -> Self {
        Self {
            id: cfg.id.clone(),
            require_mention: cfg.require_mention,
            allow_from: cfg.allow_from.clone(),
            topics: cfg.topics.iter().map(TopicConfig::from_config).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TopicConfig {
    pub id: String,
    pub require_mention: bool,
    pub allow_from: Vec<String>,
}

impl TopicConfig {
    fn from_config(cfg: &talos_config::TelegramTopicConfig) -> Self {
        Self {
            id: cfg.id.clone(),
            require_mention: cfg.require_mention,
            allow_from: cfg.allow_from.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Polling,
    Webhook,
}

fn map_dm_policy(policy: ConfigDmPolicy) -> DmPolicy {
    match policy {
        ConfigDmPolicy::Open => DmPolicy::Open,
        ConfigDmPolicy::Pairing => DmPolicy::Pairing,
        ConfigDmPolicy::Allowlist => DmPolicy::Allowlist,
    }
}

fn map_group_policy(policy: ConfigGroupPolicy) -> GroupPolicy {
    match policy {
        ConfigGroupPolicy::Open => GroupPolicy::Open,
        ConfigGroupPolicy::Mention => GroupPolicy::Mention,
        ConfigGroupPolicy::Allowlist => GroupPolicy::Allowlist,
    }
}

fn map_mode(mode: ConfigTelegramMode) -> Mode {
    match mode {
        ConfigTelegramMode::Polling => Mode::Polling,
        ConfigTelegramMode::Webhook => Mode::Webhook,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_config::{
        Config, DmPolicy as ConfigDmPolicy, GroupPolicy as ConfigGroupPolicy,
        TelegramAccountConfig, TelegramChannelsConfig, TelegramMode as ConfigTelegramMode,
    };

    #[test]
    fn test_channel_definition() {
        let channel = TelegramChannel::new();
        assert_eq!(channel.definition().id, "telegram");
        assert_eq!(channel.definition().name, "Telegram");
        assert!(channel.definition().capabilities.supports_threads);
        assert!(channel.definition().capabilities.supports_edit);
        assert!(channel.definition().capabilities.supports_typing);
    }

    #[test]
    fn test_parse_dm_policy() {
        assert_eq!(map_dm_policy(ConfigDmPolicy::Open), DmPolicy::Open);
        assert_eq!(map_dm_policy(ConfigDmPolicy::Pairing), DmPolicy::Pairing);
        assert_eq!(
            map_dm_policy(ConfigDmPolicy::Allowlist),
            DmPolicy::Allowlist
        );
    }

    #[test]
    fn test_parse_group_policy() {
        assert_eq!(map_group_policy(ConfigGroupPolicy::Open), GroupPolicy::Open);
        assert_eq!(
            map_group_policy(ConfigGroupPolicy::Mention),
            GroupPolicy::Mention
        );
        assert_eq!(
            map_group_policy(ConfigGroupPolicy::Allowlist),
            GroupPolicy::Allowlist
        );
    }

    #[test]
    fn test_parse_mode() {
        assert_eq!(map_mode(ConfigTelegramMode::Polling), Mode::Polling);
        assert_eq!(map_mode(ConfigTelegramMode::Webhook), Mode::Webhook);
    }

    #[test]
    fn test_enabled_account_ids_reads_multi_account_config() {
        let channel = TelegramChannel::new();
        let mut config = Config::default();
        config.channels.telegram = TelegramChannelsConfig {
            enabled: true,
            token: Some("root-token".to_string()),
            accounts: vec![
                TelegramAccountConfig {
                    id: "bot-a".to_string(),
                    enabled: true,
                    token: Some("token-a".to_string()),
                    ..Default::default()
                },
                TelegramAccountConfig {
                    id: "bot-b".to_string(),
                    enabled: false,
                    token: Some("token-b".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            channel.enabled_account_ids(&config),
            vec!["bot-a".to_string()]
        );
    }
}
