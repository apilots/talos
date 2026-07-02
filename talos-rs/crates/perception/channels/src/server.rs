use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use talos_config::Config;
use talos_error::{TalosError, TalosResult};
use talos_gateway::Gateway;
use talos_service::{Actor, TalosService, TimelineSubscriptionMessage, TimelineSubscriptionSource};
use talos_types::talos::{
    AgentEventPayload, TalosContext, TalosInputItem, TalosProvider, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineItemKind,
};
use talos_types::{StartTurnRequest, SubscribeTimelineRequest};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::traits::{
    Channel, ChannelEvent, ChannelRuntimeContext, MessageContent, MessageTarget, OutboundContext,
};

#[async_trait::async_trait]
pub trait ChannelTimelineSubscriptionService: Send + Sync {
    async fn timeline_subscription_source(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscriptionSource>;
}

#[async_trait::async_trait]
impl ChannelTimelineSubscriptionService for TalosService {
    async fn timeline_subscription_source(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscriptionSource> {
        TalosService::timeline_subscription_source(self, actor, request).await
    }
}

pub struct ChannelServer {
    config: Arc<Config>,
    gateway: Arc<dyn Gateway>,
    timeline_subscriptions: Arc<dyn ChannelTimelineSubscriptionService>,
    channels: HashMap<String, Box<dyn Channel>>,
    started_accounts: HashMap<String, Vec<String>>,
    event_tx: mpsc::Sender<ChannelEvent>,
    event_rx: mpsc::Receiver<ChannelEvent>,
}

impl ChannelServer {
    pub fn new(
        config: Arc<Config>,
        gateway: Arc<dyn Gateway>,
        timeline_subscriptions: Arc<dyn ChannelTimelineSubscriptionService>,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            config,
            gateway,
            timeline_subscriptions,
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
                    let request = build_start_turn_request(
                        &channel_id,
                        &account_id,
                        &user_id,
                        &chat_id,
                        thread_id.clone(),
                        session_id.0,
                        content,
                    );

                    let actor = Actor::regular(user_id);
                    match submit_channel_turn(
                        self.gateway.as_ref(),
                        self.timeline_subscriptions.as_ref(),
                        &actor,
                        request,
                    )
                    .await
                    {
                        Ok(content) => {
                            let Some(text) = normalize_channel_response(&content) else {
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
                            error!("Failed to process channel event via gateway: {}", err);
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

async fn submit_channel_turn(
    gateway: &dyn Gateway,
    timeline_subscriptions: &dyn ChannelTimelineSubscriptionService,
    actor: &Actor,
    request: StartTurnRequest,
) -> TalosResult<String> {
    let response = gateway.start_turn(actor, request, None).await?;
    let mut subscription = timeline_subscriptions
        .timeline_subscription_source(
            actor,
            SubscribeTimelineRequest {
                session_id: response.session_id.clone(),
                turn_id: Some(response.turn_id.clone()),
                since_event_id: None,
                replay_limit: Some(128),
                close_when_idle: Some(false),
                close_on_terminal: Some(true),
            },
        )
        .await?;

    let mut collector = ChannelTurnReplyCollector::default();
    let timeout = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                return Err(TalosError::InternalError(format!(
                    "timed out waiting for channel turn `{}` to complete",
                    response.turn_id
                )));
            }
            message = subscription.subscription.events.recv() => {
                let Some(message) = message else {
                    return Ok(collector.finish());
                };
                match message {
                    TimelineSubscriptionMessage::Event(record) => {
                        if record.turn_id != response.turn_id {
                            continue;
                        }
                        if collector.apply(record.event.payload) {
                            return Ok(collector.finish());
                        }
                    }
                    TimelineSubscriptionMessage::Closed(_) => {
                        return Ok(collector.finish());
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct ChannelTurnReplyCollector {
    assistant_text_items: HashMap<String, String>,
}

impl ChannelTurnReplyCollector {
    fn apply(&mut self, payload: AgentEventPayload) -> bool {
        match payload {
            AgentEventPayload::ItemStarted(started) => {
                if started.item.kind == TalosTimelineItemKind::AssistantText {
                    let text = match started.item.content {
                        TalosTimelineContent::Text { text } => text,
                        _ => String::new(),
                    };
                    self.assistant_text_items.insert(started.item.item_id, text);
                }
            }
            AgentEventPayload::ItemDelta(delta) => {
                if let TalosTimelineDelta::Text { text } = delta.delta {
                    if let Some(existing) = self.assistant_text_items.get_mut(&delta.item_id) {
                        existing.push_str(&text);
                    }
                }
            }
            AgentEventPayload::ItemCompleted(completed) => {
                if let Some(existing) = self.assistant_text_items.get_mut(&completed.item_id) {
                    if let Some(TalosTimelineContent::Text { text }) = completed.final_content {
                        *existing = text;
                    }
                }
            }
            AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnError(_) => {
                return true;
            }
            AgentEventPayload::TurnStarted(_)
            | AgentEventPayload::AssistantTextCommitted(_)
            | AgentEventPayload::TurnMetrics(_) => {}
        }
        false
    }

    fn finish(self) -> String {
        self.assistant_text_items
            .into_values()
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn build_start_turn_request(
    channel_id: &str,
    _account_id: &str,
    user_id: &str,
    chat_id: &str,
    thread_id: Option<String>,
    session_id: String,
    content: MessageContent,
) -> StartTurnRequest {
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

    StartTurnRequest {
        session_id,
        workspace_id: None,
        input,
        context: Some(context),
        model: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
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
#[path = "server_tests.rs"]
mod tests;
