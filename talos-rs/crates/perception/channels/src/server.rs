use std::collections::HashMap;
use std::sync::Arc;

use talos_config::Config;
use talos_error::TalosResult;
use talos_types::gateway::GatewayRequest;
use talos_types::talos::{TalosContext, TalosInputItem, TalosProvider};
use talos_types::RequestHandler;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::traits::{
    Channel, ChannelEvent, ChannelRuntimeContext, MessageContent, MessageTarget, OutboundContext,
};

pub struct ChannelServer {
    config: Arc<Config>,
    request_handler: Arc<dyn RequestHandler>,
    channels: HashMap<String, Box<dyn Channel>>,
    started_accounts: HashMap<String, Vec<String>>,
    event_tx: mpsc::Sender<ChannelEvent>,
    event_rx: mpsc::Receiver<ChannelEvent>,
}

impl ChannelServer {
    pub fn new(config: Arc<Config>, request_handler: Arc<dyn RequestHandler>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            config,
            request_handler,
            channels: HashMap::new(),
            started_accounts: HashMap::new(),
            event_tx,
            event_rx,
        }
    }

    pub fn register(&mut self, channel: Box<dyn Channel>) -> &mut Self {
        let id = channel.definition().id.to_string();
        self.channels.insert(id, channel);
        self
    }

    fn get_channel(&self, id: &str) -> Option<&dyn Channel> {
        self.channels.get(id).map(|channel| channel.as_ref())
    }

    #[cfg(test)]
    pub fn channel_ids(&self) -> Vec<&str> {
        self.channels.keys().map(|id| id.as_str()).collect()
    }

    pub async fn run(mut self) -> TalosResult<()> {
        self.start_enabled().await?;
        self.run_event_loop().await
    }

    async fn start_enabled(&mut self) -> TalosResult<()> {
        self.started_accounts.clear();

        for (channel_id, channel) in &self.channels {
            let account_ids = channel.enabled_account_ids(self.config.as_ref());
            let mut started = Vec::new();

            for account_id in account_ids {
                let ctx = self.create_context(account_id.clone());
                if let Err(err) = channel.start_account(ctx).await {
                    error!(
                        "Failed to start channel {} account {}: {}",
                        channel_id, account_id, err
                    );
                } else {
                    started.push(account_id);
                }
            }

            self.started_accounts.insert(channel_id.clone(), started);
        }

        Ok(())
    }

    async fn run_event_loop(&mut self) -> TalosResult<()> {
        while let Some(event) = self.event_rx.recv().await {
            match event {
                ChannelEvent::Message {
                    channel_id,
                    account_id,
                    user_id,
                    chat_id,
                    thread_id,
                    session_id,
                    content,
                    reply_to_id,
                } => {
                    let request = build_gateway_request(
                        &channel_id,
                        &account_id,
                        &user_id,
                        &chat_id,
                        thread_id.clone(),
                        session_id.0,
                        content,
                    );

                    match self.request_handler.handle(request).await {
                        Ok(response) => {
                            let Some(text) = normalize_channel_response(&response.content) else {
                                continue;
                            };
                            let Some(channel) = self.get_channel(&channel_id) else {
                                error!("Channel '{}' not found for outbound response", channel_id);
                                continue;
                            };
                            let Some(adapter) = channel.outbound_adapter() else {
                                error!(
                                    "Channel '{}' does not provide an outbound adapter",
                                    channel_id
                                );
                                continue;
                            };

                            let outbound = OutboundContext {
                                target: MessageTarget {
                                    account_id,
                                    chat_id,
                                    thread_id,
                                    reply_to: reply_to_id,
                                },
                                text,
                            };

                            if let Err(err) = adapter.send_text(&outbound).await {
                                error!("Failed to send outbound channel response: {}", err);
                            }
                        }
                        Err(err) => {
                            error!(
                                "Failed to process channel event via request handler: {}",
                                err
                            );
                        }
                    }
                }
                ChannelEvent::Error {
                    channel_id,
                    account_id,
                    error: channel_error,
                } => {
                    error!(
                        "Received channel error from {} account {}: {}",
                        channel_id, account_id, channel_error
                    );
                }
            }
        }

        info!("Channel server event loop stopped");
        Ok(())
    }

    fn create_context(&self, account_id: String) -> ChannelRuntimeContext {
        ChannelRuntimeContext {
            config: self.config.clone(),
            account_id,
            abort_signal: tokio_util::sync::CancellationToken::new(),
            event_tx: self.event_tx.clone(),
            get_status: Arc::new(|| Default::default()),
            set_status: Arc::new(|_| {}),
        }
    }
}

fn build_gateway_request(
    channel_id: &str,
    _account_id: &str,
    user_id: &str,
    chat_id: &str,
    thread_id: Option<String>,
    session_id: String,
    content: MessageContent,
) -> GatewayRequest {
    let input = match content {
        MessageContent::Text(text) => vec![TalosInputItem::Text { text }],
        MessageContent::Media { kind, file_id } => vec![TalosInputItem::Text {
            text: format!("[media:{kind:?}] {file_id}"),
        }],
    };

    let provider = match channel_id {
        "telegram" => TalosProvider::Telegram,
        _ => TalosProvider::System,
    };

    let context = {
        let context =
            TalosContext::im(provider, user_id.to_string()).with_channel_id(chat_id.to_string());
        if let Some(thread_id) = thread_id {
            context.with_thread_id(thread_id)
        } else {
            context
        }
    };

    GatewayRequest {
        session_id,
        workspace_id: None,
        input,
        context,
    }
}

fn normalize_channel_response(content: &str) -> Option<String> {
    let normalized = content.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use talos_types::gateway::{GatewayRequest, GatewayResponse};
    use talos_types::talos::{
        TalosEvent, TalosEventPayload, TalosEventType, TalosMessageStartedEvent, TalosReasoning,
        TalosUsage,
    };

    use crate::traits::{ChannelCapabilities, ChannelDefinition, DeliveryResult, OutboundAdapter};

    struct MockChannel {
        started_accounts: Mutex<Vec<String>>,
    }

    impl MockChannel {
        fn new() -> Self {
            Self {
                started_accounts: Mutex::new(Vec::new()),
            }
        }
    }

    struct MockOutboundAdapter {
        messages: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl OutboundAdapter for MockOutboundAdapter {
        async fn send_text(&self, ctx: &OutboundContext) -> TalosResult<DeliveryResult> {
            self.messages
                .lock()
                .expect("lock poisoned")
                .push(ctx.text.clone());
            Ok(DeliveryResult {
                message_id: "msg-1".to_string(),
            })
        }

        async fn send_media(
            &self,
            _ctx: &OutboundContext,
            _media: crate::traits::MediaContent,
        ) -> TalosResult<DeliveryResult> {
            Ok(DeliveryResult {
                message_id: "media-1".to_string(),
            })
        }
    }

    #[async_trait]
    impl Channel for MockChannel {
        fn definition(&self) -> &ChannelDefinition {
            static DEF: std::sync::OnceLock<ChannelDefinition> = std::sync::OnceLock::new();
            DEF.get_or_init(|| ChannelDefinition {
                id: "mock",
                name: "Mock",
                capabilities: ChannelCapabilities::default(),
            })
        }

        fn enabled_account_ids(&self, _config: &Config) -> Vec<String> {
            vec!["mock-account".to_string()]
        }

        async fn start_account(&self, ctx: ChannelRuntimeContext) -> TalosResult<()> {
            self.started_accounts
                .lock()
                .expect("lock poisoned")
                .push(ctx.account_id.clone());

            ctx.event_tx
                .send(ChannelEvent::Message {
                    channel_id: "mock".to_string(),
                    account_id: ctx.account_id,
                    user_id: "user-1".to_string(),
                    chat_id: "chat-1".to_string(),
                    thread_id: None,
                    session_id: talos_types::SessionId("session-1".to_string()),
                    content: MessageContent::Text("hello".to_string()),
                    reply_to_id: None,
                })
                .await
                .expect("event send should succeed");
            Ok(())
        }

        async fn stop_account(&self, _account_id: &str) -> TalosResult<()> {
            Ok(())
        }

        fn outbound_adapter(&self) -> Option<&dyn OutboundAdapter> {
            static EMPTY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            let _ = EMPTY.get_or_init(|| ());
            None
        }
    }

    struct MockRequestHandler;

    #[async_trait]
    impl RequestHandler for MockRequestHandler {
        async fn handle(&self, request: GatewayRequest) -> TalosResult<GatewayResponse> {
            Ok(GatewayResponse {
                content: format!("Echo: {}", extract_text_input(&request.input)),
                output_items: Vec::new(),
                reasoning: vec![TalosReasoning {
                    text: "mock reasoning".to_string(),
                }],
                stop_reason: talos_types::talos::StopReason::EndTurn,
                usage: TalosUsage::default(),
                success: true,
                model: "mock-model".to_string(),
                session_id: request.session_id,
            })
        }

        async fn handle_stream(
            &self,
            request: GatewayRequest,
        ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosEvent>> {
            let response = self.handle(request).await?;
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tokio::spawn(async move {
                let _ = tx
                    .send(TalosEvent {
                        event_type: TalosEventType::MessageStarted,
                        message_id: "msg-mock".to_string(),
                        session_id: response.session_id,
                        sequence: 0,
                        payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                            request_id: "req-mock".to_string(),
                            model: "mock-model".to_string(),
                            usage: TalosUsage::default(),
                            iteration: 0,
                            step_id: "start".to_string(),
                        }),
                    })
                    .await;
            });
            Ok(rx)
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_build_gateway_request_from_channel_event() {
        let request = build_gateway_request(
            "telegram",
            "bot-a",
            "user-1",
            "chat-1",
            Some("topic-1".to_string()),
            "session-1".to_string(),
            MessageContent::Text("hello".to_string()),
        );

        assert_eq!(extract_text_input(&request.input), "hello");
        assert_eq!(request.session_id, "session-1");

        let context = request.context;
        assert_eq!(context.provider, TalosProvider::Telegram);
        assert_eq!(context.channel_id.as_deref(), Some("chat-1"));
        assert_eq!(context.thread_id.as_deref(), Some("topic-1"));
        assert_eq!(context.user_id, "user-1");
    }

    #[test]
    fn test_normalize_channel_response() {
        assert_eq!(
            normalize_channel_response("  hello world  "),
            Some("hello world".to_string())
        );
        assert_eq!(normalize_channel_response("   "), None);
    }

    #[tokio::test]
    async fn test_channel_server_registers_channels() {
        let config = Arc::new(Config::default());
        let request_handler: Arc<dyn RequestHandler> = Arc::new(MockRequestHandler);
        let mut server = ChannelServer::new(config, request_handler);
        server.register(Box::new(MockChannel::new()));

        assert_eq!(server.channel_ids(), vec!["mock"]);
    }

    struct MockChannelWithOutbound {
        outbound: MockOutboundAdapter,
    }

    #[async_trait]
    impl Channel for MockChannelWithOutbound {
        fn definition(&self) -> &ChannelDefinition {
            static DEF: std::sync::OnceLock<ChannelDefinition> = std::sync::OnceLock::new();
            DEF.get_or_init(|| ChannelDefinition {
                id: "mock",
                name: "Mock",
                capabilities: ChannelCapabilities::default(),
            })
        }

        fn enabled_account_ids(&self, _config: &Config) -> Vec<String> {
            vec!["mock-account".to_string()]
        }

        async fn start_account(&self, ctx: ChannelRuntimeContext) -> TalosResult<()> {
            ctx.event_tx
                .send(ChannelEvent::Message {
                    channel_id: "mock".to_string(),
                    account_id: "mock-account".to_string(),
                    user_id: "user-1".to_string(),
                    chat_id: "chat-1".to_string(),
                    thread_id: None,
                    session_id: talos_types::SessionId("session-1".to_string()),
                    content: MessageContent::Text("hello".to_string()),
                    reply_to_id: None,
                })
                .await
                .expect("event send should succeed");
            Ok(())
        }

        async fn stop_account(&self, _account_id: &str) -> TalosResult<()> {
            Ok(())
        }

        fn outbound_adapter(&self) -> Option<&dyn OutboundAdapter> {
            Some(&self.outbound)
        }
    }

    #[tokio::test]
    async fn test_channel_server_processes_events_and_replies() {
        let config = Arc::new(Config::default());
        let request_handler: Arc<dyn RequestHandler> = Arc::new(MockRequestHandler);
        let outbound_messages = Arc::new(Mutex::new(Vec::new()));
        let channel = MockChannelWithOutbound {
            outbound: MockOutboundAdapter {
                messages: outbound_messages.clone(),
            },
        };
        let mut server = ChannelServer::new(config, request_handler);
        server.register(Box::new(channel));

        let run = tokio::time::timeout(std::time::Duration::from_millis(50), server.run()).await;
        assert!(run.is_err(), "server run should stay alive");
        assert_eq!(
            outbound_messages.lock().expect("lock poisoned").as_slice(),
            ["Echo: hello"]
        );
    }

    #[test]
    fn test_telegram_dm_scope_is_loaded_from_config() {
        let mut config = Config::default();
        config.channels.telegram.dm_scope = talos_config::DmScope::PerChannelPeer;
        assert_eq!(
            config.channels.telegram.dm_scope,
            talos_config::DmScope::PerChannelPeer
        );
    }

    fn extract_text_input(input: &[TalosInputItem]) -> String {
        input
            .iter()
            .filter_map(|item| match item {
                TalosInputItem::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
