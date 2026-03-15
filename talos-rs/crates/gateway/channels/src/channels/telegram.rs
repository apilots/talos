use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use talos_config::Config;
use talos_error::{TalosError, TalosResult};
use talos_gateway::Gateway;
use talos_types::canonical::CanonicalContext;
use talos_types::gateway::GatewayRequest;
use teloxide::prelude::*;
use teloxide::types::Message;
use tokio::task::JoinHandle;

use super::Channel;

pub struct ChannelsTelegram;

impl ChannelsTelegram {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Channel for ChannelsTelegram {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(
        &self,
        gateway: Arc<dyn Gateway>,
        config: &Config,
    ) -> TalosResult<Option<JoinHandle<()>>> {
        if !config.channels.telegram.enabled {
            return Ok(None);
        }

        let token = config
            .channels
            .telegram
            .token
            .clone()
            .ok_or_else(|| TalosError::ConfigError("missing telegram token".to_string()))?;
        let dm_scope = config.channels.telegram.dm_scope.clone();
        let bot = Bot::new(token);

        let handle = tokio::spawn(async move {
            let _ = teloxide::repl(bot, move |bot: Bot, msg: Message| {
                let gateway = gateway.clone();
                let dm_scope = dm_scope.clone();
                async move {
                    if let Some(text) = msg.text() {
                        let context = build_context(&msg);
                        let request = build_gateway_request(
                            context,
                            text,
                            &dm_scope,
                            msg.chat.id.0.to_string(),
                            msg.id.0.to_string(),
                        );
                        match gateway.handle_request(request).await {
                            Ok(response) => {
                                if !response.content.trim().is_empty() {
                                    let _ = bot.send_message(msg.chat.id, response.content).await;
                                }
                            }
                            Err(err) => {
                                let _ = bot
                                    .send_message(msg.chat.id, format!("Error: {}", err))
                                    .await;
                            }
                        }
                    }
                    respond(())
                }
            })
            .await;
        });

        Ok(Some(handle))
    }
}

fn build_context(msg: &Message) -> CanonicalContext {
    CanonicalContext {
        source_type: "im".to_string(),
        provider: "telegram".to_string(),
        channel_id: msg.chat.id.0.to_string(),
        user_id: msg
            .from()
            .map(|u| u.id.0.to_string())
            .unwrap_or_else(|| msg.chat.id.0.to_string()),
        thread_id: None,
        message_id: Some(msg.id.0.to_string()),
        trace_id: None,
    }
}

fn build_gateway_request(
    context: CanonicalContext,
    input: &str,
    dm_scope: &str,
    chat_id: String,
    message_id: String,
) -> GatewayRequest {
    GatewayRequest {
        channel: format!("telegram:{}", context.channel_id),
        user_id: context.user_id.clone(),
        content: input.to_string(),
        model: None,
        session_id: None,
        context: Some(context),
        metadata: Some(json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "dm_scope": dm_scope
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_name() {
        let channel = ChannelsTelegram::new();
        assert_eq!(channel.name(), "telegram");
    }

    #[test]
    fn test_build_gateway_request_contains_dm_scope() {
        let context = CanonicalContext {
            source_type: "im".to_string(),
            provider: "telegram".to_string(),
            channel_id: "1001".to_string(),
            user_id: "2002".to_string(),
            thread_id: Some("topic-1".to_string()),
            message_id: Some("3003".to_string()),
            trace_id: None,
        };
        let request = build_gateway_request(
            context,
            "hello",
            "per_channel_peer_topic",
            "1001".to_string(),
            "3003".to_string(),
        );
        let metadata = request.metadata.expect("metadata should exist");
        assert_eq!(
            metadata.get("dm_scope").and_then(|v| v.as_str()),
            Some("per_channel_peer_topic")
        );
    }
}
