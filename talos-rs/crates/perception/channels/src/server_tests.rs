use super::*;
use async_trait::async_trait;
use std::sync::Mutex;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_service::{EventReplay, TimelineCursorStatus, TimelineSubscription};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, StopReason, TalosTimelineItem,
    TalosTimelineItemStatus, TalosTimelinePhase, TalosTurnCompletedEvent, TimelineEventEnvelope,
};
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, CloudLoginCancelResponse, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse, ConfigReloadRequest,
    ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse, FsDirectoryCreateRequest,
    FsDirectoryCreateResponse, FsDirectoryReadRequest, FsDirectoryReadResponse,
    InterruptTurnRequest, InterruptTurnResponse, MemoryFileListRequest, MemoryFileListResponse,
    MemoryFileReadRequest, MemoryFileReadResponse, MemoryFileWriteRequest, MemoryFileWriteResponse,
    MemorySearchRequest, MemorySearchResponse, ModelListRequest, ModelListResponse,
    PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstallRequest, PluginInstallResponse, PluginListRequest, PluginListResponse,
    PluginReadRequest, PluginReadResponse, PluginSearchRequest, PluginSearchResponse,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse, ProfileReadRequest,
    ProfileReadResponse, ProjectCreateRequest, ProjectCreateResponse, ProjectDeleteRequest,
    ProjectDeleteResponse, ProjectListRequest, ProjectListResponse, ProjectPatchRequest,
    ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse, RecoverTimelineRequest,
    RetryTurnRequest, RetryTurnResponse, SessionActiveTurnRequest, SessionActiveTurnResponse,
    SessionClearRequest, SessionClearResponse, SessionCloseRequest, SessionCloseResponse,
    SessionCompactRequest, SessionCompactResponse, SessionDeleteRequest, SessionDeleteResponse,
    SessionGetRequest, SessionGetResponse, SessionHistoryRequest, SessionHistoryResponse,
    SessionListRequest, SessionListResponse, SessionPatchRequest, SessionUpdateResponse,
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsReadResponse,
    SkillsRefreshRequest, SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest,
    SkillsSuggestResponse, StartTurnResponse, SubmitDynamicToolResultRequest,
    SubmitDynamicToolResultResponse, SubscribeTimelineResponse, TimelineRecoveryResponse,
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
    async fn read_profile(
        &self,
        _actor: &Actor,
        _request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse> {
        unused_gateway("read_profile")
    }

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

    async fn submit_dynamic_tool_result(
        &self,
        _actor: &Actor,
        _request: SubmitDynamicToolResultRequest,
    ) -> TalosResult<SubmitDynamicToolResultResponse> {
        unreachable!("submit_dynamic_tool_result is not used by channel tests")
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
        _persistence: talos_auth::CloudLoginPersistence,
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
    async fn timeline_subscription_source(
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
                    AgentEventType::ItemStarted,
                    session_id.clone(),
                    AgentEventPayload::ItemStarted(talos_types::talos::TalosItemStartedEvent {
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
                    }),
                )))
                .await;
            let _ = tx
                .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                    "event-2",
                    AgentEventType::ItemDelta,
                    session_id.clone(),
                    AgentEventPayload::ItemDelta(talos_types::talos::TalosItemDeltaEvent {
                        request_id: "req-mock".to_string(),
                        turn_id: "turn-mock".to_string(),
                        item_id: "item-1".to_string(),
                        delta: TalosTimelineDelta::Text {
                            text: "hello".to_string(),
                        },
                    }),
                )))
                .await;
            let _ = tx
                .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                    "event-3",
                    AgentEventType::TurnCompleted,
                    session_id,
                    AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
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
    event_type: AgentEventType,
    session_id: String,
    payload: AgentEventPayload,
) -> TimelineEventEnvelope {
    TimelineEventEnvelope {
        event_id: event_id.to_string(),
        event_seq: 1,
        created_at_ms: 0,
        session_id: session_id.clone(),
        turn_id: "turn-mock".to_string(),
        message_id: "msg-mock".to_string(),
        event: AgentEvent {
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
