use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use talos_agent_api::{
    AgentAssistantTextPhase, AgentEvent, AgentEventPayload, AgentMessageBlockContent,
    AgentMessageRole,
};
use talos_config::Config;
use talos_error::{TalosError, TalosResult};
use talos_service::{Actor, TalosService, TimelineSubscription, TimelineSubscriptionMessage};
use talos_types::talos::{SessionSource, SessionSourceProvider};
use talos_types::{AgentUserContent, AgentUserInput};
use talos_types::{StartTurnRequest, SubscribeTimelineRequest};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::traits::{
    Channel, ChannelEvent, ChannelRuntimeContext, MessageContent, MessageTarget, OutboundContext,
};

/// Exposes the product operations required to process one inbound Channel message.
///
/// Implementations own Turn admission and timeline subscription. Channel runtimes
/// must not depend on transport gateways or the Agent execution framework.
pub trait ChannelApplicationService: Send + Sync {
    fn start_turn(
        &self,
        actor: &Actor,
        source: SessionSource,
        request: StartTurnRequest,
    ) -> impl std::future::Future<Output = TalosResult<talos_types::StartTurnResponse>> + Send;

    fn open_timeline_subscription(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> impl std::future::Future<Output = TalosResult<TimelineSubscription>> + Send;
}

impl ChannelApplicationService for TalosService {
    async fn start_turn(
        &self,
        actor: &Actor,
        source: SessionSource,
        request: StartTurnRequest,
    ) -> TalosResult<talos_types::StartTurnResponse> {
        TalosService::start_turn(self, actor, source, request, None).await
    }

    async fn open_timeline_subscription(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscription> {
        TalosService::open_timeline_subscription(self, actor, request).await
    }
}

pub struct ChannelServer<S = TalosService> {
    config: Arc<Config>,
    application: Arc<S>,
    channels: HashMap<String, Box<dyn Channel>>,
    started_accounts: HashMap<String, Vec<String>>,
    event_tx: mpsc::Sender<ChannelEvent>,
    event_rx: mpsc::Receiver<ChannelEvent>,
}

impl<S> ChannelServer<S>
where
    S: ChannelApplicationService,
{
    pub fn new(config: Arc<Config>, application: Arc<S>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            config,
            application,
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
                    let (source, request) = build_start_turn_request(
                        &channel_id,
                        &account_id,
                        &user_id,
                        &chat_id,
                        thread_id.clone(),
                        session_id.0,
                        content,
                    );

                    let actor = Actor::regular(user_id);
                    match submit_channel_turn(self.application.as_ref(), &actor, source, request)
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
                            error!("Failed to process channel event: {}", err);
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
            get_status: Arc::new(Default::default),
            set_status: Arc::new(|_| {}),
        }
    }
}

async fn submit_channel_turn(
    application: &impl ChannelApplicationService,
    actor: &Actor,
    source: SessionSource,
    request: StartTurnRequest,
) -> TalosResult<String> {
    let response = application.start_turn(actor, source, request).await?;
    let mut subscription = application
        .open_timeline_subscription(
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
                return Err(TalosError::internal("Timed out waiting for channel turn to complete.").context(format!(
                    "timed out waiting for channel turn `{}` to complete",
                    response.turn_id
                )));
            }
            message = subscription.events.recv() => {
                let Some(message) = message else {
                    return Ok(collector.finish());
                };
                match message {
                    TimelineSubscriptionMessage::Event(record) => {
                        if record.event.identity().turn_id != response.turn_id {
                            continue;
                        }
                        let Some(event) = record.event.agent_event().cloned() else { continue; };
                        if collector.apply(event) {
                            return Ok(collector.finish());
                        }
                    }
                    TimelineSubscriptionMessage::MessageQueueChanged(_) => {}
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
    text: Vec<String>,
}

impl ChannelTurnReplyCollector {
    fn apply(&mut self, event: AgentEvent) -> bool {
        let terminal = matches!(
            &event.payload,
            AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_)
        );
        if let AgentEventPayload::MessageCompleted(message) = event.payload {
            self.collect_message(message);
        }
        terminal
    }

    fn finish(self) -> String {
        self.text.join("\n")
    }

    fn collect_message(&mut self, message: talos_agent_api::AgentMessage) {
        if message.role != AgentMessageRole::Agent {
            return;
        }
        for block in message.blocks {
            let AgentMessageBlockContent::AssistantText(assistant) = block.content else {
                continue;
            };
            if assistant.phase != AgentAssistantTextPhase::Pending
                && !assistant.text.trim().is_empty()
            {
                self.text.push(assistant.text);
            }
        }
    }
}

fn build_start_turn_request(
    channel_id: &str,
    _account_id: &str,
    _user_id: &str,
    chat_id: &str,
    thread_id: Option<String>,
    session_id: String,
    content: MessageContent,
) -> (SessionSource, StartTurnRequest) {
    let input = AgentUserInput {
        content: vec![match content {
            MessageContent::Text(text) => AgentUserContent::Text { text },
            MessageContent::Media { kind, file_id } => AgentUserContent::Text {
                text: format!("[media:{kind:?}] {file_id}"),
            },
        }],
    };

    let provider = match channel_id {
        "telegram" => SessionSourceProvider::Telegram,
        _ => SessionSourceProvider::System,
    };

    let source = {
        let source = SessionSource::im(provider).with_channel_id(chat_id.to_string());
        if let Some(thread_id) = thread_id {
            source.with_thread_id(thread_id)
        } else {
            source
        }
    };

    (
        source,
        StartTurnRequest {
            session_id,
            input,
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
    )
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
