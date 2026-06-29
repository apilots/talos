//! Telegram Webhook mode implementation.
//!
//! This module provides webhook-based update receiving for Telegram bots.
//! It integrates with external HTTP servers (like axum) to receive updates.

use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::types::Update;
use tracing::{error, info, warn};

use crate::policy::PolicyChecker;
use crate::traits::{ChannelEvent, ChannelRuntimeContext};
use crate::types::{AccessDecision, MessageContent};

use super::bot::{AccountConfig, GroupConfig, TopicConfig};

/// Webhook handler for processing Telegram updates.
#[derive(Clone)]
#[allow(dead_code)]
pub struct TelegramWebhookHandler {
    config: AccountConfig,
    bot: Bot,
    ctx: ChannelRuntimeContext,
    policy_checker: PolicyChecker,
}

#[allow(dead_code)]
impl TelegramWebhookHandler {
    pub fn new(config: AccountConfig, ctx: ChannelRuntimeContext) -> Self {
        let bot = Bot::new(&config.token);

        let policy_checker = PolicyChecker::new(
            config.dm_policy,
            config.group_policy,
            config.allow_from.clone(),
            config.group_allow_from.clone(),
        );

        Self {
            config,
            bot,
            ctx,
            policy_checker,
        }
    }

    pub fn account_id(&self) -> &str {
        &self.config.id
    }

    pub fn webhook_url(&self) -> Option<&str> {
        self.config.webhook_url.as_deref()
    }

    /// Processes an incoming update from Telegram.
    pub async fn handle_update(&self, update: Update) {
        let msg = match update.kind {
            teloxide::types::UpdateKind::Message(ref msg) => msg,
            _ => return,
        };

        let text = match msg.text() {
            Some(t) => t,
            None => return,
        };

        if let Err(e) = self.process_message(msg, text).await {
            error!("Failed to process message: {}", e);
        }
    }

    async fn process_message(
        &self,
        msg: &Message,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let user_id = msg
            .from()
            .map(|u| u.id.0.to_string())
            .unwrap_or_else(|| msg.chat.id.0.to_string());

        let chat_id = msg.chat.id.0.to_string();
        let thread_id = msg.thread_id.map(|id| id.to_string());
        let is_group = msg.chat.is_group() || msg.chat.is_supergroup();
        let is_mentioned = text.contains('@');

        let access = if is_group {
            let group_config = self.config.groups.iter().find(|g| g.id == chat_id);
            check_group_access(
                &self.policy_checker,
                &chat_id,
                is_mentioned,
                group_config,
                &thread_id,
            )
        } else {
            self.policy_checker.check_dm_access(&user_id, false)
        };

        match access {
            AccessDecision::Denied => {
                info!("Access denied for user {} in chat {}", user_id, chat_id);
                return Ok(());
            }
            AccessDecision::NeedPairing => {
                self.bot
                    .send_message(msg.chat.id, "Please pair with the bot first using /start")
                    .await?;
                return Ok(());
            }
            AccessDecision::Allowed => {}
        }

        let session_id = build_session_id(&chat_id, thread_id.as_deref(), &user_id);

        let event = ChannelEvent::Message {
            channel_id: "telegram".to_string(),
            account_id: self.config.id.clone(),
            user_id,
            chat_id,
            thread_id,
            session_id,
            content: MessageContent::Text(text.to_string()),
            reply_to_id: msg.reply_to_message().map(|m| m.id.0.to_string()),
        };

        self.ctx.event_tx.send(event).await?;

        Ok(())
    }
}

fn check_group_access(
    policy_checker: &PolicyChecker,
    chat_id: &str,
    is_mentioned: bool,
    group_config: Option<&GroupConfig>,
    thread_id: &Option<String>,
) -> AccessDecision {
    match group_config {
        Some(group) => {
            if let Some(ref tid) = thread_id {
                let topic_config = group.topics.iter().find(|t| &t.id == tid);
                if let Some(topic) = topic_config {
                    return check_topic_access(topic, is_mentioned);
                }
            }
            check_group_config_access(group, is_mentioned)
        }
        None => policy_checker.check_group_access(chat_id, is_mentioned),
    }
}

fn check_group_config_access(group: &GroupConfig, is_mentioned: bool) -> AccessDecision {
    if group.require_mention && !is_mentioned {
        AccessDecision::Denied
    } else {
        AccessDecision::Allowed
    }
}

fn check_topic_access(topic: &TopicConfig, is_mentioned: bool) -> AccessDecision {
    if topic.require_mention && !is_mentioned {
        AccessDecision::Denied
    } else {
        AccessDecision::Allowed
    }
}

fn build_session_id(
    chat_id: &str,
    thread_id: Option<&str>,
    user_id: &str,
) -> talos_types::SessionId {
    let key = match thread_id {
        Some(tid) => format!("telegram:{}:{}:{}", chat_id, tid, user_id),
        None => format!("telegram:{}:{}", chat_id, user_id),
    };
    talos_types::SessionId(key)
}

/// Webhook manager for multiple Telegram accounts.
#[allow(dead_code)]
pub struct WebhookManager {
    handlers: Vec<Arc<TelegramWebhookHandler>>,
}

#[allow(dead_code)]
impl WebhookManager {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn register_handler(&mut self, handler: TelegramWebhookHandler) {
        self.handlers.push(Arc::new(handler));
    }

    pub fn get_handler(&self, account_id: &str) -> Option<Arc<TelegramWebhookHandler>> {
        self.handlers
            .iter()
            .find(|h| h.account_id() == account_id)
            .cloned()
    }

    pub fn account_ids(&self) -> Vec<&str> {
        self.handlers.iter().map(|h| h.account_id()).collect()
    }

    pub async fn setup_webhooks(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for handler in &self.handlers {
            if let Some(url) = handler.webhook_url() {
                info!(
                    "Setting up webhook for account {}: {}",
                    handler.account_id(),
                    url
                );

                let bot = &handler.bot;
                bot.set_webhook(url.parse()?).send().await.map_err(|e| {
                    warn!("Failed to set webhook for {}: {}", handler.account_id(), e);
                    e
                })?;

                info!(
                    "Webhook set successfully for account {}",
                    handler.account_id()
                );
            } else {
                warn!(
                    "No webhook URL configured for account {}, skipping webhook setup",
                    handler.account_id()
                );
            }
        }
        Ok(())
    }

    pub async fn teardown_webhooks(&self) {
        for handler in &self.handlers {
            if let Err(e) = handler.bot.delete_webhook().send().await {
                warn!(
                    "Failed to delete webhook for {}: {}",
                    handler.account_id(),
                    e
                );
            } else {
                info!("Webhook deleted for account {}", handler.account_id());
            }
        }
    }
}

impl Default for WebhookManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "webhook_tests.rs"]
mod tests;
