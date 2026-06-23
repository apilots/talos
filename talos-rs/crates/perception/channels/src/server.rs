use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use talos_config::Config;
use talos_error::{TalosError, TalosResult};
use talos_gateway::Gateway;
use talos_service::{Actor, TalosService, TimelineSubscriptionMessage, TimelineSubscriptionSource};
use talos_types::talos::{
    TalosContext, TalosInputItem, TalosProvider, TalosTimelineContent, TalosTimelineDelta,
    TalosTimelineEventPayload, TalosTimelineItemKind,
};
use talos_types::{StartTurnRequest, SubscribeTimelineRequest};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::traits::{
    Channel, ChannelEvent, ChannelRuntimeContext, MessageContent, MessageTarget, OutboundContext,
};

#[async_trait::async_trait]
pub trait ChannelTimelineSubscriptionService: Send + Sync {
    async fn open_timeline_subscription(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscriptionSource>;
}

#[async_trait::async_trait]
impl ChannelTimelineSubscriptionService for TalosService {
    async fn open_timeline_subscription(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscriptionSource> {
        TalosService::open_timeline_subscription(self, actor, request).await
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
        .open_timeline_subscription(
            actor,
            SubscribeTimelineRequest {
                session_id: response.session_id.clone(),
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
    fn apply(&mut self, payload: TalosTimelineEventPayload) -> bool {
        match payload {
            TalosTimelineEventPayload::ItemStarted(started) => {
                if started.item.kind == TalosTimelineItemKind::AssistantText {
                    let text = match started.item.content {
                        TalosTimelineContent::Text { text } => text,
                        _ => String::new(),
                    };
                    self.assistant_text_items.insert(started.item.item_id, text);
                }
            }
            TalosTimelineEventPayload::ItemDelta(delta) => {
                if let TalosTimelineDelta::Text { text } = delta.delta {
                    if let Some(existing) = self.assistant_text_items.get_mut(&delta.item_id) {
                        existing.push_str(&text);
                    }
                }
            }
            TalosTimelineEventPayload::ItemCompleted(completed) => {
                if let Some(existing) = self.assistant_text_items.get_mut(&completed.item_id) {
                    if let Some(TalosTimelineContent::Text { text }) = completed.final_content {
                        *existing = text;
                    }
                }
            }
            TalosTimelineEventPayload::TurnCompleted(_)
            | TalosTimelineEventPayload::TurnError(_) => {
                return true;
            }
            TalosTimelineEventPayload::TurnStarted(_)
            | TalosTimelineEventPayload::AssistantTextCommitted(_)
            | TalosTimelineEventPayload::TurnMetrics(_) => {}
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
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use talos_auth::{
        CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
        CloudLoginStatusRequest, CloudLoginStatusResponse,
    };
    use talos_service::{EventReplay, TimelineCursorStatus, TimelineSubscription};
    use talos_types::talos::{
        StopReason, TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineEventType,
        TalosTimelineItem, TalosTimelineItemStatus, TalosTimelinePhase, TalosTurnCompletedEvent,
        TimelineEventEnvelope,
    };
    use talos_types::{
        AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
        AuthSessionReadRequest, AuthSessionReadResponse, AuthSessionStoreRequest,
        AuthSessionStoreResponse, CloudLoginCancelResponse, ConfigDeleteRequest,
        ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse, ConfigReloadRequest,
        ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse, FsDirectoryCreateRequest,
        FsDirectoryCreateResponse, FsDirectoryReadRequest, FsDirectoryReadResponse,
        InterruptTurnRequest, InterruptTurnResponse, MemoryFileListRequest, MemoryFileListResponse,
        MemoryFileReadRequest, MemoryFileReadResponse, MemoryFileWriteRequest,
        MemoryFileWriteResponse, MemorySearchRequest, MemorySearchResponse, ModelListRequest,
        ModelListResponse, PluginDisableRequest, PluginDisableResponse, PluginEnableRequest,
        PluginEnableResponse, PluginInstallRequest, PluginInstallResponse, PluginListRequest,
        PluginListResponse, PluginReadRequest, PluginReadResponse, PluginSearchRequest,
        PluginSearchResponse, PluginSourceAddRequest, PluginSourceAddResponse,
        PluginSourceListRequest, PluginSourceListResponse, PluginSourceRefreshRequest,
        PluginSourceRefreshResponse, PluginSourceRemoveRequest, PluginSourceRemoveResponse,
        PluginUninstallRequest, PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
        ProjectCreateRequest, ProjectCreateResponse, ProjectDeleteRequest, ProjectDeleteResponse,
        ProjectListRequest, ProjectListResponse, ProjectPatchRequest, ProjectPatchResponse,
        ProjectReadRequest, ProjectReadResponse, RecoverTimelineRequest, RetryTurnRequest,
        RetryTurnResponse, SessionActiveTurnRequest, SessionActiveTurnResponse,
        SessionClearRequest, SessionClearResponse, SessionCloseRequest, SessionCloseResponse,
        SessionCompactRequest, SessionCompactResponse, SessionDeleteRequest, SessionDeleteResponse,
        SessionGetRequest, SessionGetResponse, SessionHistoryRequest, SessionHistoryResponse,
        SessionListRequest, SessionListResponse, SessionPatchRequest, SessionUpdateResponse,
        SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsReadResponse,
        SkillsRefreshRequest, SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest,
        SkillsSuggestResponse, StartTurnResponse, SubmitToolResultRequest,
        SubmitToolResultResponse, SubscribeTimelineResponse, TimelineRecoveryResponse,
        TurnCommandStatus, UserCurrentRequest, UserCurrentResponse, WorkspaceDocListRequest,
        WorkspaceDocListResponse, WorkspaceDocReadRequest, WorkspaceDocReadResponse,
        WorkspaceDocWriteRequest, WorkspaceDocWriteResponse, WorkspaceInfoRequest,
        WorkspaceInfoResponse,
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

    struct MockGateway;
    struct MockTimelineSubscriptions;

    fn unused_gateway<T>(method: &str) -> TalosResult<T> {
        unreachable!("{method} is not used by channel tests")
    }

    #[async_trait]
    impl Gateway for MockGateway {
        async fn start_turn(
            &self,
            _actor: &Actor,
            request: StartTurnRequest,
            _idempotency_key: Option<String>,
        ) -> TalosResult<StartTurnResponse> {
            Ok(StartTurnResponse {
                request_id: "req-mock".to_string(),
                message_id: Some("msg-mock".to_string()),
                session_id: request.session_id,
                turn_id: "turn-mock".to_string(),
                status: TurnCommandStatus::Accepted,
            })
        }

        async fn retry_turn(
            &self,
            _actor: &Actor,
            _request: RetryTurnRequest,
        ) -> TalosResult<RetryTurnResponse> {
            unreachable!("retry_turn is not used by channel tests")
        }

        async fn interrupt_turn(
            &self,
            _actor: &Actor,
            _request: InterruptTurnRequest,
        ) -> TalosResult<InterruptTurnResponse> {
            unreachable!("interrupt_turn is not used by channel tests")
        }

        async fn submit_tool_result(
            &self,
            _actor: &Actor,
            _request: SubmitToolResultRequest,
        ) -> TalosResult<SubmitToolResultResponse> {
            unreachable!("submit_tool_result is not used by channel tests")
        }

        async fn subscribe_timeline(
            &self,
            _actor: &Actor,
            request: SubscribeTimelineRequest,
        ) -> TalosResult<SubscribeTimelineResponse> {
            Ok(SubscribeTimelineResponse {
                subscription_id: "sub-mock".to_string(),
                session_id: request.session_id,
                high_water_mark_event_id: None,
            })
        }

        async fn recover_timeline(
            &self,
            _actor: &Actor,
            _request: RecoverTimelineRequest,
        ) -> TalosResult<TimelineRecoveryResponse> {
            unreachable!("recover_timeline is not used by channel tests")
        }

        async fn list_sessions(
            &self,
            _actor: &Actor,
            _request: SessionListRequest,
        ) -> TalosResult<SessionListResponse> {
            unreachable!("list_sessions is not used by channel tests")
        }

        async fn get_session(
            &self,
            _actor: &Actor,
            _request: SessionGetRequest,
        ) -> TalosResult<SessionGetResponse> {
            unreachable!("get_session is not used by channel tests")
        }

        async fn get_session_history(
            &self,
            _actor: &Actor,
            _request: SessionHistoryRequest,
        ) -> TalosResult<SessionHistoryResponse> {
            unreachable!("get_session_history is not used by channel tests")
        }

        async fn get_active_turn(
            &self,
            _actor: &Actor,
            _request: SessionActiveTurnRequest,
        ) -> TalosResult<SessionActiveTurnResponse> {
            unreachable!("get_active_turn is not used by channel tests")
        }

        async fn patch_session(
            &self,
            _actor: &Actor,
            _request: SessionPatchRequest,
        ) -> TalosResult<SessionUpdateResponse> {
            unreachable!("patch_session is not used by channel tests")
        }

        async fn close_session(
            &self,
            _actor: &Actor,
            _request: SessionCloseRequest,
        ) -> TalosResult<SessionCloseResponse> {
            unreachable!("close_session is not used by channel tests")
        }

        async fn delete_session(
            &self,
            _actor: &Actor,
            _request: SessionDeleteRequest,
        ) -> TalosResult<SessionDeleteResponse> {
            unreachable!("delete_session is not used by channel tests")
        }

        async fn compact_session(
            &self,
            _actor: &Actor,
            _request: SessionCompactRequest,
        ) -> TalosResult<SessionCompactResponse> {
            unreachable!("compact_session is not used by channel tests")
        }

        async fn clear_session(
            &self,
            _actor: &Actor,
            _request: SessionClearRequest,
        ) -> TalosResult<SessionClearResponse> {
            unreachable!("clear_session is not used by channel tests")
        }

        async fn list_projects(
            &self,
            _actor: &Actor,
            _request: ProjectListRequest,
        ) -> TalosResult<ProjectListResponse> {
            unreachable!("list_projects is not used by channel tests")
        }

        async fn get_project(
            &self,
            _actor: &Actor,
            _request: ProjectReadRequest,
        ) -> TalosResult<ProjectReadResponse> {
            unreachable!("get_project is not used by channel tests")
        }

        async fn create_project(
            &self,
            _actor: &Actor,
            _request: ProjectCreateRequest,
        ) -> TalosResult<ProjectCreateResponse> {
            unreachable!("create_project is not used by channel tests")
        }

        async fn patch_project(
            &self,
            _actor: &Actor,
            _request: ProjectPatchRequest,
        ) -> TalosResult<ProjectPatchResponse> {
            unreachable!("patch_project is not used by channel tests")
        }

        async fn delete_project(
            &self,
            _actor: &Actor,
            _request: ProjectDeleteRequest,
        ) -> TalosResult<ProjectDeleteResponse> {
            unreachable!("delete_project is not used by channel tests")
        }

        async fn read_fs_directory(
            &self,
            _actor: &Actor,
            _request: FsDirectoryReadRequest,
        ) -> TalosResult<FsDirectoryReadResponse> {
            unreachable!("read_fs_directory is not used by channel tests")
        }

        async fn create_fs_directory(
            &self,
            _actor: &Actor,
            _request: FsDirectoryCreateRequest,
        ) -> TalosResult<FsDirectoryCreateResponse> {
            unreachable!("create_fs_directory is not used by channel tests")
        }

        async fn list_models(
            &self,
            _actor: &Actor,
            _request: ModelListRequest,
        ) -> TalosResult<ModelListResponse> {
            unreachable!("list_models is not used by channel tests")
        }

        async fn get_config(
            &self,
            _actor: &Actor,
            _request: ConfigGetRequest,
        ) -> TalosResult<ConfigGetResponse> {
            unused_gateway("get_config")
        }

        async fn update_config(
            &self,
            _actor: &Actor,
            _request: ConfigUpdateRequest,
        ) -> TalosResult<ConfigUpdateResponse> {
            unused_gateway("update_config")
        }

        async fn delete_config(
            &self,
            _actor: &Actor,
            _request: ConfigDeleteRequest,
        ) -> TalosResult<ConfigDeleteResponse> {
            unused_gateway("delete_config")
        }

        async fn reload_config(
            &self,
            _actor: &Actor,
            _request: ConfigReloadRequest,
        ) -> TalosResult<ConfigReloadResponse> {
            unused_gateway("reload_config")
        }

        async fn list_skills(
            &self,
            _actor: &Actor,
            _request: SkillsListRequest,
        ) -> TalosResult<SkillsListResponse> {
            unused_gateway("list_skills")
        }

        async fn refresh_skills(
            &self,
            _actor: &Actor,
            _request: SkillsRefreshRequest,
        ) -> TalosResult<SkillsListResponse> {
            unused_gateway("refresh_skills")
        }

        async fn suggest_skills(
            &self,
            _actor: &Actor,
            _request: SkillsSuggestRequest,
        ) -> TalosResult<SkillsSuggestResponse> {
            unused_gateway("suggest_skills")
        }

        async fn read_skill(
            &self,
            _actor: &Actor,
            _request: SkillsReadRequest,
        ) -> TalosResult<SkillsReadResponse> {
            unused_gateway("read_skill")
        }

        async fn resolve_skills(
            &self,
            _actor: &Actor,
            _request: SkillsResolveRequest,
        ) -> TalosResult<SkillsResolveResponse> {
            unused_gateway("resolve_skills")
        }

        async fn list_plugin_sources(
            &self,
            _actor: &Actor,
            _request: PluginSourceListRequest,
        ) -> TalosResult<PluginSourceListResponse> {
            unused_gateway("list_plugin_sources")
        }

        async fn add_plugin_source(
            &self,
            _actor: &Actor,
            _request: PluginSourceAddRequest,
        ) -> TalosResult<PluginSourceAddResponse> {
            unused_gateway("add_plugin_source")
        }

        async fn refresh_plugin_source(
            &self,
            _actor: &Actor,
            _request: PluginSourceRefreshRequest,
        ) -> TalosResult<PluginSourceRefreshResponse> {
            unused_gateway("refresh_plugin_source")
        }

        async fn remove_plugin_source(
            &self,
            _actor: &Actor,
            _request: PluginSourceRemoveRequest,
        ) -> TalosResult<PluginSourceRemoveResponse> {
            unused_gateway("remove_plugin_source")
        }

        async fn search_plugins(
            &self,
            _actor: &Actor,
            _request: PluginSearchRequest,
        ) -> TalosResult<PluginSearchResponse> {
            unused_gateway("search_plugins")
        }

        async fn list_plugins(
            &self,
            _actor: &Actor,
            _request: PluginListRequest,
        ) -> TalosResult<PluginListResponse> {
            unused_gateway("list_plugins")
        }

        async fn read_plugin(
            &self,
            _actor: &Actor,
            _request: PluginReadRequest,
        ) -> TalosResult<PluginReadResponse> {
            unused_gateway("read_plugin")
        }

        async fn install_plugin(
            &self,
            _actor: &Actor,
            _request: PluginInstallRequest,
        ) -> TalosResult<PluginInstallResponse> {
            unused_gateway("install_plugin")
        }

        async fn update_plugin(
            &self,
            _actor: &Actor,
            _request: PluginUpdateRequest,
        ) -> TalosResult<PluginUpdateResponse> {
            unused_gateway("update_plugin")
        }

        async fn uninstall_plugin(
            &self,
            _actor: &Actor,
            _request: PluginUninstallRequest,
        ) -> TalosResult<PluginUninstallResponse> {
            unused_gateway("uninstall_plugin")
        }

        async fn enable_plugin(
            &self,
            _actor: &Actor,
            _request: PluginEnableRequest,
        ) -> TalosResult<PluginEnableResponse> {
            unused_gateway("enable_plugin")
        }

        async fn disable_plugin(
            &self,
            _actor: &Actor,
            _request: PluginDisableRequest,
        ) -> TalosResult<PluginDisableResponse> {
            unused_gateway("disable_plugin")
        }

        async fn list_memory_files(
            &self,
            _actor: &Actor,
            _request: MemoryFileListRequest,
        ) -> TalosResult<MemoryFileListResponse> {
            unused_gateway("list_memory_files")
        }

        async fn read_memory_file(
            &self,
            _actor: &Actor,
            _request: MemoryFileReadRequest,
        ) -> TalosResult<MemoryFileReadResponse> {
            unused_gateway("read_memory_file")
        }

        async fn write_memory_file(
            &self,
            _actor: &Actor,
            _request: MemoryFileWriteRequest,
        ) -> TalosResult<MemoryFileWriteResponse> {
            unused_gateway("write_memory_file")
        }

        async fn search_memory(
            &self,
            _actor: &Actor,
            _request: MemorySearchRequest,
        ) -> TalosResult<MemorySearchResponse> {
            unused_gateway("search_memory")
        }

        async fn list_workspace_docs(
            &self,
            _actor: &Actor,
            _request: WorkspaceDocListRequest,
        ) -> TalosResult<WorkspaceDocListResponse> {
            unused_gateway("list_workspace_docs")
        }

        async fn read_workspace_doc(
            &self,
            _actor: &Actor,
            _request: WorkspaceDocReadRequest,
        ) -> TalosResult<WorkspaceDocReadResponse> {
            unused_gateway("read_workspace_doc")
        }

        async fn write_workspace_doc(
            &self,
            _actor: &Actor,
            _request: WorkspaceDocWriteRequest,
        ) -> TalosResult<WorkspaceDocWriteResponse> {
            unused_gateway("write_workspace_doc")
        }

        async fn workspace_info(
            &self,
            _actor: &Actor,
            _request: WorkspaceInfoRequest,
        ) -> TalosResult<WorkspaceInfoResponse> {
            unused_gateway("workspace_info")
        }

        async fn logout(
            &self,
            _actor: &Actor,
            _request: AuthLogoutRequest,
        ) -> TalosResult<AuthLogoutResponse> {
            unused_gateway("logout")
        }

        async fn start_cloud_login(
            &self,
            _actor: &Actor,
            _request: CloudLoginStartRequest,
        ) -> TalosResult<CloudLoginStartResponse> {
            unused_gateway("start_cloud_login")
        }

        async fn cloud_login_status(
            &self,
            _actor: &Actor,
            _request: CloudLoginStatusRequest,
        ) -> TalosResult<CloudLoginStatusResponse> {
            unused_gateway("cloud_login_status")
        }

        async fn cancel_cloud_login(
            &self,
            _actor: &Actor,
            _request: CloudLoginCancelRequest,
        ) -> TalosResult<CloudLoginCancelResponse> {
            unused_gateway("cancel_cloud_login")
        }

        async fn read_auth_session(
            &self,
            _actor: &Actor,
            _request: AuthSessionReadRequest,
        ) -> TalosResult<AuthSessionReadResponse> {
            unused_gateway("read_auth_session")
        }

        async fn store_auth_session(
            &self,
            _actor: &Actor,
            _request: AuthSessionStoreRequest,
        ) -> TalosResult<AuthSessionStoreResponse> {
            unused_gateway("store_auth_session")
        }

        async fn clear_auth_session(
            &self,
            _actor: &Actor,
            _request: AuthSessionClearRequest,
        ) -> TalosResult<AuthSessionClearResponse> {
            unused_gateway("clear_auth_session")
        }

        async fn current_user(
            &self,
            _actor: &Actor,
            _request: UserCurrentRequest,
        ) -> TalosResult<UserCurrentResponse> {
            unused_gateway("current_user")
        }

        async fn start(&self) -> TalosResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ChannelTimelineSubscriptionService for MockTimelineSubscriptions {
        async fn open_timeline_subscription(
            &self,
            _actor: &Actor,
            request: SubscribeTimelineRequest,
        ) -> TalosResult<TimelineSubscriptionSource> {
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            let session_id = request.session_id.clone();
            tokio::spawn(async move {
                let _ = tx
                    .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                        "event-1",
                        TalosTimelineEventType::ItemStarted,
                        session_id.clone(),
                        TalosTimelineEventPayload::ItemStarted(
                            talos_types::talos::TalosItemStartedEvent {
                                request_id: "req-mock".to_string(),
                                turn_id: "turn-mock".to_string(),
                                item: TalosTimelineItem {
                                    item_id: "item-1".to_string(),
                                    parent_item_id: None,
                                    kind: TalosTimelineItemKind::AssistantText,
                                    phase: TalosTimelinePhase::FinalAnswer,
                                    status: TalosTimelineItemStatus::Running,
                                    title: None,
                                    started_at_ms: 0,
                                    completed_at_ms: None,
                                    content: TalosTimelineContent::Text {
                                        text: "Echo: ".to_string(),
                                    },
                                },
                            },
                        ),
                    )))
                    .await;
                let _ = tx
                    .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                        "event-2",
                        TalosTimelineEventType::ItemDelta,
                        session_id.clone(),
                        TalosTimelineEventPayload::ItemDelta(
                            talos_types::talos::TalosItemDeltaEvent {
                                request_id: "req-mock".to_string(),
                                turn_id: "turn-mock".to_string(),
                                item_id: "item-1".to_string(),
                                delta: TalosTimelineDelta::Text {
                                    text: "hello".to_string(),
                                },
                            },
                        ),
                    )))
                    .await;
                let _ = tx
                    .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                        "event-3",
                        TalosTimelineEventType::TurnCompleted,
                        session_id,
                        TalosTimelineEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                            request_id: "req-mock".to_string(),
                            turn_id: "turn-mock".to_string(),
                            completed_at_ms: 0,
                            stop_reason: StopReason::EndTurn,
                            usage: Default::default(),
                            metrics: None,
                        }),
                    )))
                    .await;
            });
            Ok(TimelineSubscriptionSource {
                response: SubscribeTimelineResponse {
                    subscription_id: "sub-mock".to_string(),
                    session_id: request.session_id.clone(),
                    high_water_mark_event_id: None,
                },
                subscription: TimelineSubscription {
                    session_id: request.session_id,
                    high_water_mark_event_id: None,
                    replay: EventReplay {
                        events: Vec::new(),
                        high_water_mark_event_id: None,
                        last_replay_seq: 0,
                        cursor_status: TimelineCursorStatus::Current,
                    },
                    events: rx,
                    has_live_turn: true,
                },
            })
        }
    }

    #[test]
    fn test_build_start_turn_request_from_channel_event() {
        let request = build_start_turn_request(
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

        let context = request.context.expect("context should be set");
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
        let gateway: Arc<dyn Gateway> = Arc::new(MockGateway);
        let subscriptions: Arc<dyn ChannelTimelineSubscriptionService> =
            Arc::new(MockTimelineSubscriptions);
        let mut server = ChannelServer::new(config, gateway, subscriptions);
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
        let gateway: Arc<dyn Gateway> = Arc::new(MockGateway);
        let subscriptions: Arc<dyn ChannelTimelineSubscriptionService> =
            Arc::new(MockTimelineSubscriptions);
        let outbound_messages = Arc::new(Mutex::new(Vec::new()));
        let channel = MockChannelWithOutbound {
            outbound: MockOutboundAdapter {
                messages: outbound_messages.clone(),
            },
        };
        let mut server = ChannelServer::new(config, gateway, subscriptions);
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

    fn timeline_envelope(
        event_id: &str,
        event_type: TalosTimelineEventType,
        session_id: String,
        payload: TalosTimelineEventPayload,
    ) -> TimelineEventEnvelope {
        TimelineEventEnvelope {
            event_id: event_id.to_string(),
            event_seq: 1,
            created_at_ms: 0,
            session_id: session_id.clone(),
            turn_id: "turn-mock".to_string(),
            message_id: "msg-mock".to_string(),
            event: TalosTimelineEvent {
                event_seq: Some(1),
                event_id: Some(event_id.to_string()),
                created_at_ms: Some(0),
                event_type,
                turn_id: "turn-mock".to_string(),
                message_id: "msg-mock".to_string(),
                session_id,
                sequence: 1,
                payload,
            },
        }
    }
}
