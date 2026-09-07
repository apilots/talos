use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;
use teloxide::prelude::*;
use tokio::sync::RwLock;

use crate::traits::{DeliveryResult, MediaContent, OutboundAdapter, OutboundContext};

pub struct TelegramOutboundAdapter {
    bots: Arc<RwLock<HashMap<String, Bot>>>,
}

impl TelegramOutboundAdapter {
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

impl Default for TelegramOutboundAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OutboundAdapter for TelegramOutboundAdapter {
    async fn send_text(&self, ctx: &OutboundContext) -> TalosResult<DeliveryResult> {
        let bots = self.bots.read().await;
        let bot = bots
            .get(&ctx.target.account_id)
            .ok_or_else(|| talos_error::TalosError::internal("Bot not found"))?;

        let chat_id = Self::parse_chat_id(&ctx.target.chat_id);

        let mut request = bot.send_message(chat_id, &ctx.text);

        if let Some(thread_id) = &ctx.target.thread_id {
            if let Ok(tid) = thread_id.parse::<i32>() {
                request = request.message_thread_id(tid);
            }
        }

        if let Some(reply_to) = &ctx.target.reply_to {
            if let Ok(reply_to_id) = reply_to.parse::<i32>() {
                request = request.reply_to_message_id(teloxide::types::MessageId(reply_to_id));
            }
        }

        let sent = request.await.map_err(|e| {
            talos_error::TalosError::internal_with_source(
                "Failed to send message; technical details were withheld.",
                e,
            )
        })?;

        Ok(DeliveryResult {
            message_id: sent.id.0.to_string(),
        })
    }

    async fn send_media(
        &self,
        ctx: &OutboundContext,
        media: MediaContent,
    ) -> TalosResult<DeliveryResult> {
        let bots = self.bots.read().await;
        let bot = bots
            .get(&ctx.target.account_id)
            .ok_or_else(|| talos_error::TalosError::internal("Bot not found"))?;

        let chat_id = Self::parse_chat_id(&ctx.target.chat_id);
        let sent = match media.kind {
            crate::types::MediaType::Photo => {
                let mut request =
                    bot.send_photo(chat_id, teloxide::types::InputFile::file_id(&media.file_id));
                if let Some(caption) = media.caption.as_deref() {
                    request = request.caption(caption.to_string());
                }
                if let Some(thread_id) = &ctx.target.thread_id {
                    if let Ok(tid) = thread_id.parse::<i32>() {
                        request = request.message_thread_id(tid);
                    }
                }
                request.await
            }
            crate::types::MediaType::Document => {
                let mut request =
                    bot.send_document(chat_id, teloxide::types::InputFile::file_id(&media.file_id));
                if let Some(caption) = media.caption.as_deref() {
                    request = request.caption(caption.to_string());
                }
                if let Some(thread_id) = &ctx.target.thread_id {
                    if let Ok(tid) = thread_id.parse::<i32>() {
                        request = request.message_thread_id(tid);
                    }
                }
                request.await
            }
            crate::types::MediaType::Voice => {
                let mut request =
                    bot.send_voice(chat_id, teloxide::types::InputFile::file_id(&media.file_id));
                if let Some(caption) = media.caption.as_deref() {
                    request = request.caption(caption.to_string());
                }
                if let Some(thread_id) = &ctx.target.thread_id {
                    if let Ok(tid) = thread_id.parse::<i32>() {
                        request = request.message_thread_id(tid);
                    }
                }
                request.await
            }
            crate::types::MediaType::Video => {
                let mut request =
                    bot.send_video(chat_id, teloxide::types::InputFile::file_id(&media.file_id));
                if let Some(caption) = media.caption.as_deref() {
                    request = request.caption(caption.to_string());
                }
                if let Some(thread_id) = &ctx.target.thread_id {
                    if let Ok(tid) = thread_id.parse::<i32>() {
                        request = request.message_thread_id(tid);
                    }
                }
                request.await
            }
        }
        .map_err(|e| {
            talos_error::TalosError::internal_with_source(
                "Failed to send media; technical details were withheld.",
                e,
            )
        })?;

        Ok(DeliveryResult {
            message_id: sent.id.0.to_string(),
        })
    }
}
