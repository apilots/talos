use super::*;
use async_trait::async_trait;
use std::sync::Mutex;
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEvent, AgentEventPayload, AgentMessage,
    AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus, AgentMessageRole,
    AgentMessageStatus, AgentStopReason, AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
    TimelineCursorStatus, TimelineEvent, TimelineReplay,
};
use talos_service::TimelineSubscription;
use talos_types::{StartTurnResponse, TurnCommandStatus};

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

struct MockApplicationService;

impl ChannelApplicationService for MockApplicationService {
    async fn start_turn(
        &self,
        _actor: &Actor,
        _source: SessionSource,
        request: StartTurnRequest,
    ) -> TalosResult<StartTurnResponse> {
        Ok(StartTurnResponse {
            request_id: "req-mock".to_string(),
            message_id: Some("msg-mock".to_string()),
            session_id: request.session_id,
            turn_id: "turn-mock".to_string(),
            status: TurnCommandStatus::Accepted,
        })
    }
    async fn open_timeline_subscription(
        &self,
        _actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscription> {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let session_id = request.session_id.clone();
        tokio::spawn(async move {
            let identity = AgentTurnRef::new(session_id, "turn-mock");
            let events = [
                AgentEvent {
                    identity: identity.clone(),
                    sequence: 1,
                    occurred_at: 1,
                    payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
                },
                AgentEvent {
                    identity: identity.clone(),
                    sequence: 2,
                    occurred_at: 2,
                    payload: AgentEventPayload::MessageCompleted(AgentMessage {
                        message_id: "message-mock".to_string(),
                        turn_id: identity.turn_id.clone(),
                        role: AgentMessageRole::Agent,
                        status: AgentMessageStatus::Completed,
                        blocks: vec![AgentMessageBlock {
                            block_id: "block-mock".to_string(),
                            status: AgentMessageBlockStatus::Completed,
                            content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
                                phase: AgentAssistantTextPhase::FinalAnswer,
                                text: "Echo: hello".to_string(),
                            }),
                            started_at: 1,
                            completed_at: Some(2),
                        }],
                        created_at: 1,
                        completed_at: Some(2),
                    }),
                },
                AgentEvent {
                    identity,
                    sequence: 3,
                    occurred_at: 3,
                    payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                        stop_reason: AgentStopReason::EndTurn,
                    }),
                },
            ];
            for event in events {
                let _ = tx
                    .send(TimelineSubscriptionMessage::Event(Box::new(
                        timeline_event(event),
                    )))
                    .await;
            }
        });
        Ok(TimelineSubscription {
            subscription_id: "sub-mock".to_string(),
            session_id: request.session_id,
            high_water_mark_event_id: None,
            replay: TimelineReplay {
                events: Vec::new(),
                high_water_mark_event_id: None,
                next_cursor: None,
                last_event_seq: 0,
                cursor_status: TimelineCursorStatus::Current,
            },
            events: rx,
            has_live_turn: true,
        })
    }
}

#[test]
fn test_build_start_turn_request_from_channel_event() {
    let (source, request) = build_start_turn_request(
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

    assert_eq!(source.source_provider, SessionSourceProvider::Telegram);
    assert_eq!(source.channel_id.as_deref(), Some("chat-1"));
    assert_eq!(source.thread_id.as_deref(), Some("topic-1"));
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
    let application = Arc::new(MockApplicationService);
    let mut server = ChannelServer::new(config, application);
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
    let application = Arc::new(MockApplicationService);
    let outbound_messages = Arc::new(Mutex::new(Vec::new()));
    let channel = MockChannelWithOutbound {
        outbound: MockOutboundAdapter {
            messages: outbound_messages.clone(),
        },
    };
    let mut server = ChannelServer::new(config, application);
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

fn extract_text_input(input: &AgentUserInput) -> String {
    input
        .content
        .iter()
        .filter_map(|item| match item {
            AgentUserContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn timeline_event(event: AgentEvent) -> TimelineEvent {
    TimelineEvent {
        event_id: format!("event-{}", event.sequence),
        event_seq: event.sequence as i64,
        persisted_at: event.occurred_at,
        event: talos_agent_api::TimelineEventPayload::Agent(event),
    }
}
