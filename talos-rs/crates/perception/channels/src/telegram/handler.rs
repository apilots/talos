use teloxide::prelude::*;
use tracing::{error, info};

use crate::policy::PolicyChecker;
use crate::traits::{ChannelEvent, ChannelRuntimeContext};
use crate::types::{AccessDecision, MessageContent};

use super::bot::{AccountConfig, GroupConfig, TopicConfig};

pub struct TelegramMessageHandler {
    config: AccountConfig,
    bot: Bot,
    ctx: ChannelRuntimeContext,
    policy_checker: PolicyChecker,
}

impl TelegramMessageHandler {
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

    pub async fn run(self) {
        let policy_checker = self.policy_checker.clone();
        let event_tx = self.ctx.event_tx.clone();
        let account_id = self.config.id.clone();
        let groups = self.config.groups.clone();

        teloxide::repl(self.bot, move |bot: Bot, msg: Message| {
            let policy_checker = policy_checker.clone();
            let event_tx = event_tx.clone();
            let account_id = account_id.clone();
            let groups = groups.clone();
            async move {
                if let Some(text) = msg.text() {
                    if let Err(e) = Self::handle_message(
                        bot,
                        &msg,
                        text,
                        &policy_checker,
                        &event_tx,
                        &account_id,
                        &groups,
                    )
                    .await
                    {
                        error!("Failed to handle message: {}", e);
                    }
                }
                respond(())
            }
        })
        .await;
    }

    async fn handle_message(
        bot: Bot,
        msg: &Message,
        text: &str,
        policy_checker: &PolicyChecker,
        event_tx: &tokio::sync::mpsc::Sender<ChannelEvent>,
        account_id: &str,
        groups: &[GroupConfig],
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
            let group_config = groups.iter().find(|g| g.id == chat_id);
            check_group_access(
                policy_checker,
                &chat_id,
                is_mentioned,
                group_config,
                &thread_id,
            )
        } else {
            policy_checker.check_dm_access(&user_id, false)
        };

        match access {
            AccessDecision::Denied => {
                info!("Access denied for user {} in chat {}", user_id, chat_id);
                return Ok(());
            }
            AccessDecision::NeedPairing => {
                bot.send_message(msg.chat.id, "Please pair with the bot first using /start")
                    .await?;
                return Ok(());
            }
            AccessDecision::Allowed => {}
        }

        let session_id = build_session_id(&chat_id, thread_id.as_deref(), &user_id);

        let event = ChannelEvent::Message {
            channel_id: "telegram".to_string(),
            account_id: account_id.to_string(),
            user_id,
            chat_id,
            thread_id,
            session_id,
            content: MessageContent::Text(text.to_string()),
            reply_to_id: msg.reply_to_message().map(|m| m.id.0.to_string()),
        };

        event_tx.send(event).await?;

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
