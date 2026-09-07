use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;
use teloxide::prelude::*;
use teloxide::types::MessageId;
use tokio::sync::RwLock;

use crate::traits::{MessageTarget, StreamingAdapter};

pub struct TelegramStreamingAdapter {
    bots: Arc<RwLock<HashMap<String, Bot>>>,
}

impl TelegramStreamingAdapter {
    pub fn new() -> Self {
        Self {
            bots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_bot(&self, account_id: String, token: String) {
        let bot = Bot::new(&token);
        self.bots.write().await.insert(account_id, bot);
    }

    fn parse_chat_id(chat_id: &str) -> ChatId {
        chat_id.parse::<i64>().map(ChatId).unwrap_or(ChatId(0))
    }
}

impl Default for TelegramStreamingAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StreamingAdapter for TelegramStreamingAdapter {
    async fn send_typing(&self, target: &MessageTarget) -> TalosResult<()> {
        let bots = self.bots.read().await;
        let bot = bots
            .get(&target.account_id)
            .ok_or_else(|| talos_error::TalosError::internal("Bot not found"))?;

        let chat_id = Self::parse_chat_id(&target.chat_id);

        bot.send_chat_action(chat_id, teloxide::types::ChatAction::Typing)
            .await
            .map_err(|e| {
                talos_error::TalosError::internal_with_source(
                    "Failed to send typing; technical details were withheld.",
                    e,
                )
            })?;

        Ok(())
    }

    async fn edit_text(
        &self,
        target: &MessageTarget,
        message_id: &str,
        text: &str,
    ) -> TalosResult<()> {
        let bots = self.bots.read().await;
        let bot = bots
            .get(&target.account_id)
            .ok_or_else(|| talos_error::TalosError::internal("Bot not found"))?;

        let chat_id = Self::parse_chat_id(&target.chat_id);
        let msg_id: i32 = message_id.parse().unwrap_or(0);

        bot.edit_message_text(chat_id, MessageId(msg_id), text)
            .await
            .map_err(|e| {
                talos_error::TalosError::internal_with_source(
                    "Failed to edit message; technical details were withheld.",
                    e,
                )
            })?;

        Ok(())
    }
}
