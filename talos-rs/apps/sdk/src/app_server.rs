use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;
use talos_app_protocol::events::TimelineSubscriptionNotification;
use talos_app_protocol::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, RequestId};
use talos_app_protocol::requests::{
    ActorCredential, ActorCredentials, ClientCapabilities, ClientInfo, InitializeParams,
    InitializeResponse, SkillsListRequest, SkillsListResponse, SkillsReadRequest,
    SkillsRefreshRequest, SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest,
    SkillsSuggestResponse,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginClientKind, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionReadRequest, AuthSessionReadResponse,
    CloudLoginCancelResponse, ConfigGetRequest, ConfigGetResponse, ConfigUpdateRequest,
    ConfigUpdateResponse, ConfigValue, EventUnsubscribeRequest, EventUnsubscribeResponse,
    InterruptTurnRequest, InterruptTurnResponse, LlmConfig, MemoryFileListRequest,
    MemoryFileListResponse, MemoryFileReadRequest, MemoryFileReadResponse, MemoryFileWriteRequest,
    MemoryFileWriteResponse, ModelListRequest, ModelListResponse, PluginDisableRequest,
    PluginDisableResponse, PluginEnableRequest, PluginEnableResponse, PluginInstallRequest,
    PluginInstallResponse, PluginListRequest, PluginListResponse, PluginReadRequest,
    PluginReadResponse, PluginSearchRequest, PluginSearchResponse, PluginSourceAddRequest,
    PluginSourceAddResponse, PluginSourceListRequest, PluginSourceListResponse,
    PluginSourceRefreshRequest, PluginSourceRefreshResponse, PluginSourceRemoveRequest,
    PluginSourceRemoveResponse, PluginUninstallRequest, PluginUninstallResponse,
    PluginUpdateRequest, PluginUpdateResponse, ProjectCreateRequest, ProjectCreateResponse,
    ProjectDeleteRequest, ProjectDeleteResponse, ProjectListRequest, ProjectListResponse,
    ProjectPatchRequest, ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse,
    RecoverTimelineRequest, SessionActiveTurnRequest, SessionActiveTurnResponse, SessionAgentConf,
    SessionCloseRequest, SessionCloseResponse, SessionConf, SessionDeleteRequest,
    SessionDeleteResponse, SessionGetRequest, SessionGetResponse, SessionHistoryRequest,
    SessionHistoryResponse, SessionListRequest, SessionListResponse, SessionPatchRequest,
    SessionUpdateResponse, SkillsReadResponse, StartTurnRequest, StartTurnResponse,
    SubmitToolResultRequest, SubmitToolResultResponse, SubscribeTimelineRequest,
    SubscribeTimelineResponse, TimelineRecoveryResponse, UserCurrentResponse,
    WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocReadResponse, WorkspaceDocWriteRequest, WorkspaceDocWriteResponse,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::app_server_events::is_terminal_application_event;
use crate::common::expected_file_version_token;
use crate::ExpectedFileVersion;
use crate::TalosInputItem;
use crate::WorkspaceDocKind;
use talos_types::talos::TimelineEventEnvelope;

#[derive(Debug, Clone)]
pub struct AppServerClientConfig {
    pub app_server_bin: String,
    pub client_name: String,
    pub client_version: String,
    pub actor_credentials: Option<ActorCredentials>,
    pub use_cloud_session_cache: bool,
}

impl AppServerClientConfig {
    pub fn from_env() -> TalosResult<Self> {
        Ok(Self {
            app_server_bin: std::env::var("TALOS_APP_SERVER_BIN")
                .unwrap_or_else(|_| "app-server".to_string()),
            client_name: "talos-rust-sdk".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            actor_credentials: None,
            use_cloud_session_cache: false,
        })
    }

    pub fn with_cloud_session_cache(mut self) -> Self {
        self.use_cloud_session_cache = true;
        self
    }
}

pub struct AppServerClient {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    next_id: i32,
    pending_responses: Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    subscription_senders: Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    events: mpsc::Receiver<TimelineEventEnvelope>,
    notifications: mpsc::Receiver<TimelineSubscriptionNotification>,
    actor_credentials: Option<ActorCredentials>,
    use_cloud_session_cache: bool,
}

struct SubscriptionStream {
    session_id: String,
    sender: mpsc::Sender<TimelineEventEnvelope>,
    turn_id: Option<String>,
    pending: Vec<TimelineEventEnvelope>,
}

const APP_SERVER_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl AppServerClient {
    pub async fn connect(config: AppServerClientConfig) -> TalosResult<Self> {
        let mut client = Self::spawn(&config).await?;
        client.initialize(&config).await?;
        client.notify_initialized().await?;
        if config.use_cloud_session_cache {
            client.restore_cloud_session_candidate().await?;
        }
        Ok(client)
    }

    async fn spawn(config: &AppServerClientConfig) -> TalosResult<Self> {
        let mut child = Command::new(&config.app_server_bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                TalosError::ClientError(format!("spawn {}: {error}", config.app_server_bin))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TalosError::ClientError("app-server stdin is not piped".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TalosError::ClientError("app-server stdout is not piped".to_string()))?;
        let pending_responses = Arc::new(Mutex::new(HashMap::new()));
        let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
        let (events_tx, events_rx) = mpsc::channel(1024);
        let (notifications_tx, notifications_rx) = mpsc::channel(1024);
        spawn_stdout_reader(
            stdout,
            pending_responses.clone(),
            subscription_senders.clone(),
            events_tx,
            notifications_tx,
        );
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            next_id: 1,
            pending_responses,
            subscription_senders,
            events: events_rx,
            notifications: notifications_rx,
            actor_credentials: config.actor_credentials.clone(),
            use_cloud_session_cache: config.use_cloud_session_cache,
        })
    }

    async fn initialize(
        &mut self,
        config: &AppServerClientConfig,
    ) -> TalosResult<InitializeResponse> {
        self.request_typed(
            "initialize",
            InitializeParams {
                client_info: ClientInfo {
                    name: config.client_name.clone(),
                    title: None,
                    version: config.client_version.clone(),
                },
                capabilities: ClientCapabilities {
                    protocol_version: ProtocolVersion::CURRENT,
                    supports_server_requests: false,
                    supports_event_replay: true,
                },
                actor_credentials: None,
            },
        )
        .await
    }

    async fn notify_initialized(&mut self) -> TalosResult<()> {
        let notification = JsonRpcNotification::new("initialized", None);
        self.write_json(&notification).await
    }

    pub async fn restore_cloud_session_candidate(&mut self) -> TalosResult<bool> {
        let response: AuthSessionReadResponse = self
            .request_typed("auth.session.read", AuthSessionReadRequest {})
            .await?;
        if let Some(session) = response.session {
            self.actor_credentials = Some(ActorCredentials {
                credential: ActorCredential::ExistingSessionToken {
                    token: session.access_token,
                },
            });
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn read_auth_session(&mut self) -> TalosResult<AuthSessionReadResponse> {
        self.request_typed("auth.session.read", AuthSessionReadRequest {})
            .await
    }

    pub async fn start_cloud_login(
        &mut self,
        client_name: impl Into<String>,
        client_kind: CloudLoginClientKind,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.request_typed(
            "auth.login.start",
            CloudLoginStartRequest {
                client_name: client_name.into(),
                client_kind,
            },
        )
        .await
    }

    pub async fn poll_cloud_login(
        &mut self,
        login_id: impl Into<String>,
    ) -> TalosResult<CloudLoginStatusResponse> {
        let response: CloudLoginStatusResponse = self
            .request_typed(
                "auth.login.status",
                CloudLoginStatusRequest {
                    login_id: login_id.into(),
                },
            )
            .await?;
        if matches!(response, CloudLoginStatusResponse::Authenticated { .. }) {
            let _ = self.restore_cloud_session_candidate().await?;
        }
        Ok(response)
    }

    pub async fn cancel_cloud_login(
        &mut self,
        login_id: impl Into<String>,
    ) -> TalosResult<CloudLoginCancelResponse> {
        self.request_typed(
            "auth.login.cancel",
            CloudLoginCancelRequest {
                login_id: login_id.into(),
            },
        )
        .await
    }

    pub async fn current_user(&mut self) -> TalosResult<UserCurrentResponse> {
        self.request_typed("user.current", serde_json::json!({}))
            .await
    }

    pub async fn auth_logout(&mut self) -> TalosResult<AuthLogoutResponse> {
        let response = self
            .request_typed("auth.logout", AuthLogoutRequest::default())
            .await?;
        self.actor_credentials = None;
        Ok(response)
    }

    pub async fn list_models(&mut self) -> TalosResult<ModelListResponse> {
        self.request_typed("model.list", ModelListRequest::default())
            .await
    }

    pub async fn list_skills(
        &mut self,
        params: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.request_typed("skills.list", params).await
    }

    pub async fn refresh_skills(
        &mut self,
        params: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.request_typed("skills.refresh", params).await
    }

    pub async fn suggest_skills(
        &mut self,
        params: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        self.request_typed("skills.suggest", params).await
    }

    pub async fn read_skill(
        &mut self,
        params: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        self.request_typed("skills.read", params).await
    }

    pub async fn resolve_skills(
        &mut self,
        params: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        self.request_typed("skills.resolve", params).await
    }

    pub async fn list_plugin_sources(
        &mut self,
        params: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.request_typed("plugin.source.list", params).await
    }

    pub async fn add_plugin_source(
        &mut self,
        params: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.request_typed("plugin.source.add", params).await
    }

    pub async fn refresh_plugin_source(
        &mut self,
        params: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.request_typed("plugin.source.refresh", params).await
    }

    pub async fn remove_plugin_source(
        &mut self,
        params: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.request_typed("plugin.source.remove", params).await
    }

    pub async fn search_plugins(
        &mut self,
        params: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        self.request_typed("plugin.search", params).await
    }

    pub async fn list_plugins(
        &mut self,
        params: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        self.request_typed("plugin.list", params).await
    }

    pub async fn read_plugin(
        &mut self,
        params: PluginReadRequest,
    ) -> TalosResult<PluginReadResponse> {
        self.request_typed("plugin.read", params).await
    }

    pub async fn install_plugin(
        &mut self,
        params: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        self.request_typed("plugin.install", params).await
    }

    pub async fn update_plugin(
        &mut self,
        params: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        self.request_typed("plugin.update", params).await
    }

    pub async fn uninstall_plugin(
        &mut self,
        params: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.request_typed("plugin.uninstall", params).await
    }

    pub async fn enable_plugin(
        &mut self,
        params: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        self.request_typed("plugin.enable", params).await
    }

    pub async fn disable_plugin(
        &mut self,
        params: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        self.request_typed("plugin.disable", params).await
    }

    pub async fn config_get(&mut self) -> TalosResult<ConfigGetResponse> {
        self.request_typed("config.get", ConfigGetRequest::default())
            .await
    }

    pub async fn get_llm_config(&mut self) -> TalosResult<ConfigGetResponse> {
        self.request_typed("config.get", ConfigGetRequest::default())
            .await
    }

    pub async fn update_llm_config(
        &mut self,
        config: LlmConfig,
    ) -> TalosResult<ConfigUpdateResponse> {
        self.request_typed(
            "config.update",
            ConfigUpdateRequest {
                path: "llm".to_string(),
                value: ConfigValue::Llm(config),
            },
        )
        .await
    }

    pub async fn list_memory_files(&mut self) -> TalosResult<MemoryFileListResponse> {
        self.request_typed(
            "memory.file.list",
            MemoryFileListRequest {
                limit: Some(32),
                before_date: None,
                date_from: None,
                date_to: None,
            },
        )
        .await
    }

    pub async fn read_memory_file(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> TalosResult<MemoryFileReadResponse> {
        self.request_typed(
            "memory.file.read",
            MemoryFileReadRequest { path: path.into() },
        )
        .await
    }

    pub async fn write_memory_file(
        &mut self,
        path: impl Into<PathBuf>,
        content: String,
        expected_version: Option<String>,
    ) -> TalosResult<MemoryFileWriteResponse> {
        self.request_typed(
            "memory.file.write",
            MemoryFileWriteRequest {
                path: path.into(),
                content,
                expected_version,
            },
        )
        .await
    }

    pub async fn list_workspace_docs(&mut self) -> TalosResult<WorkspaceDocListResponse> {
        self.request_typed("workspace.doc.list", WorkspaceDocListRequest {})
            .await
    }

    pub async fn read_workspace_doc(
        &mut self,
        kind: WorkspaceDocKind,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        self.request_typed("workspace.doc.read", WorkspaceDocReadRequest { kind })
            .await
    }

    pub async fn write_workspace_doc(
        &mut self,
        kind: WorkspaceDocKind,
        content: String,
        expected_version: ExpectedFileVersion,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        self.request_typed(
            "workspace.doc.write",
            WorkspaceDocWriteRequest {
                kind,
                content,
                expected_version: expected_file_version_token(expected_version),
            },
        )
        .await
    }

    pub async fn list_sessions(
        &mut self,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        self.request_typed("session.list", request).await
    }

    pub async fn get_session(
        &mut self,
        request: SessionGetRequest,
    ) -> TalosResult<SessionGetResponse> {
        self.request_typed("session.get", request).await
    }

    pub async fn get_session_by_id(&mut self, session_id: &str) -> TalosResult<SessionGetResponse> {
        self.get_session(SessionGetRequest {
            session_id: session_id.to_string(),
        })
        .await
    }

    pub async fn get_session_history(
        &mut self,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        self.request_typed("session.history", request).await
    }

    pub async fn get_session_history_by_id(
        &mut self,
        session_id: &str,
    ) -> TalosResult<SessionHistoryResponse> {
        self.get_session_history(SessionHistoryRequest {
            session_id: session_id.to_string(),
        })
        .await
    }

    pub async fn get_active_turn(
        &mut self,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        self.request_typed("session.active_turn", request).await
    }

    pub async fn get_active_turn_by_id(
        &mut self,
        session_id: &str,
    ) -> TalosResult<SessionActiveTurnResponse> {
        self.get_active_turn(SessionActiveTurnRequest {
            session_id: session_id.to_string(),
        })
        .await
    }

    pub async fn delete_session(
        &mut self,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        self.request_typed("session.delete", request).await
    }

    pub async fn delete_session_by_id(
        &mut self,
        session_id: &str,
    ) -> TalosResult<SessionDeleteResponse> {
        self.delete_session(SessionDeleteRequest {
            session_id: session_id.to_string(),
        })
        .await
    }

    pub async fn close_session(
        &mut self,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        self.request_typed("session.close", request).await
    }

    pub async fn close_session_by_id(
        &mut self,
        session_id: &str,
    ) -> TalosResult<SessionCloseResponse> {
        self.close_session(SessionCloseRequest {
            session_id: session_id.to_string(),
        })
        .await
    }

    pub async fn recover_session(
        &mut self,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse> {
        self.request_typed("session.recover", request).await
    }

    pub async fn recover_session_after(
        &mut self,
        session_id: &str,
        after_event_id: Option<String>,
    ) -> TalosResult<TimelineRecoveryResponse> {
        self.recover_session(RecoverTimelineRequest {
            session_id: session_id.to_string(),
            after_event_id,
            replay_limit: None,
        })
        .await
    }

    pub async fn list_projects(
        &mut self,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        self.request_typed::<_, ProjectListResponse>("project.list", request)
            .await
    }

    pub async fn list_all_projects(&mut self) -> TalosResult<ProjectListResponse> {
        self.list_projects(ProjectListRequest::default()).await
    }

    pub async fn get_project(
        &mut self,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        self.request_typed::<_, ProjectReadResponse>("project.read", request)
            .await
    }

    pub async fn get_project_by_id(
        &mut self,
        project_id: &str,
    ) -> TalosResult<ProjectReadResponse> {
        self.get_project(ProjectReadRequest {
            project_id: project_id.to_string(),
        })
        .await
    }

    pub async fn create_project(
        &mut self,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        self.request_typed::<_, ProjectCreateResponse>("project.create", request)
            .await
    }

    pub async fn patch_project(
        &mut self,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        self.request_typed::<_, ProjectPatchResponse>("project.patch", request)
            .await
    }

    pub async fn update_project_name(
        &mut self,
        project_id: &str,
        name: impl Into<String>,
    ) -> TalosResult<ProjectPatchResponse> {
        self.patch_project(ProjectPatchRequest {
            project_id: project_id.to_string(),
            name: Some(name.into()),
        })
        .await
    }

    pub async fn delete_project(
        &mut self,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        self.request_typed("project.delete", request).await
    }

    pub async fn delete_project_by_id(
        &mut self,
        project_id: &str,
    ) -> TalosResult<ProjectDeleteResponse> {
        self.delete_project(ProjectDeleteRequest {
            project_id: project_id.to_string(),
        })
        .await
    }

    pub async fn patch_session(
        &mut self,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        self.request_typed("session.patch", request).await
    }

    pub async fn update_session_model(
        &mut self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<SessionGetResponse> {
        let _ = self
            .patch_session(SessionPatchRequest {
                session_id: session_id.to_string(),
                conf: Some(SessionConf {
                    agent: Some(SessionAgentConf {
                        model: Some(model.to_string()),
                    }),
                }),
                project_id: None,
                current_directory: None,
            })
            .await?;
        self.get_session_by_id(session_id).await
    }

    pub async fn start_turn(
        &mut self,
        mut request: StartTurnRequest,
    ) -> TalosResult<StartTurnResponse> {
        request.input = validate_app_server_input(request.input)?;
        self.request_typed("turn.start", request).await
    }

    pub async fn interrupt_turn(
        &mut self,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.request_typed("turn.interrupt", request).await
    }

    pub async fn interrupt_turn_by_id(
        &mut self,
        session_id: &str,
    ) -> TalosResult<InterruptTurnResponse> {
        self.interrupt_turn(InterruptTurnRequest {
            session_id: session_id.to_string(),
        })
        .await
    }

    pub async fn submit_dynamic_tool_result_from_fields(
        &mut self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<SubmitToolResultResponse> {
        self.submit_tool_result(SubmitToolResultRequest {
            session_id: session_id.to_string(),
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            output,
            success,
        })
        .await
    }

    pub async fn submit_tool_result(
        &mut self,
        request: SubmitToolResultRequest,
    ) -> TalosResult<SubmitToolResultResponse> {
        self.request_typed("tool.dynamic_result.submit", request)
            .await
    }

    pub async fn subscribe_events(
        &mut self,
        params: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse> {
        self.request_typed("event.subscribe", params).await
    }

    pub async fn unsubscribe_events(
        &mut self,
        subscription_id: &str,
    ) -> TalosResult<EventUnsubscribeResponse> {
        self.subscription_senders
            .lock()
            .await
            .remove(subscription_id);
        self.request_typed(
            "event.unsubscribe",
            EventUnsubscribeRequest {
                subscription_id: subscription_id.to_string(),
            },
        )
        .await
    }

    pub async fn next_event(&mut self) -> TalosResult<TimelineEventEnvelope> {
        self.events
            .recv()
            .await
            .ok_or_else(|| TalosError::ClientError("app-server event stream closed".to_string()))
    }

    pub async fn next_notification(&mut self) -> TalosResult<TimelineSubscriptionNotification> {
        self.notifications.recv().await.ok_or_else(|| {
            TalosError::ClientError("app-server notification stream closed".to_string())
        })
    }

    pub async fn send_timeline_events(
        &mut self,
        mut request: StartTurnRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let session_id = request.session_id.clone();
        if request.client_request_id.is_none() {
            request.client_request_id =
                Some(format!("app-server-sdk-{}", uuid::Uuid::new_v4().simple()));
        }
        let start_response = self.start_turn(request).await?;
        let subscription = self
            .subscribe_events(send_timeline_subscription_params(&session_id))
            .await?;
        let subscription_id = subscription.subscription_id;
        let (tx, rx) = mpsc::channel(256);
        self.subscription_senders.lock().await.insert(
            subscription_id.clone(),
            SubscriptionStream {
                session_id: session_id.clone(),
                sender: tx,
                turn_id: Some(start_response.turn_id),
                pending: Vec::new(),
            },
        );
        Ok(rx)
    }

    pub async fn reconnect_active_timeline_events(
        &mut self,
        session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let active_turn = self.get_active_turn_by_id(session_id).await?.active_turn;
        let Some(turn_id) = active_turn.turn_id else {
            return Err(TalosError::Conflict(
                "No active turn stream is available for this session.".to_string(),
            ));
        };
        let subscription = self
            .subscribe_events(SubscribeTimelineRequest {
                session_id: session_id.to_string(),
                since_event_id,
                replay_limit: Some(256),
                close_when_idle: Some(false),
                close_on_terminal: Some(true),
            })
            .await?;
        let subscription_id = subscription.subscription_id;
        let (tx, rx) = mpsc::channel(256);
        self.subscription_senders.lock().await.insert(
            subscription_id,
            SubscriptionStream {
                session_id: session_id.to_string(),
                sender: tx,
                turn_id: Some(turn_id),
                pending: Vec::new(),
            },
        );
        Ok(rx)
    }

    pub async fn shutdown(&mut self) -> TalosResult<()> {
        let stdin = self.stdin.take();
        let child = self.child.take();

        if let Some(mut stdin) = stdin {
            stdin
                .shutdown()
                .await
                .map_err(|error| TalosError::ClientError(error.to_string()))?;
        }
        if let Some(mut child) = child {
            let status = child
                .wait()
                .await
                .map_err(|error| TalosError::ClientError(error.to_string()))?;
            if status.success() {
                Ok(())
            } else {
                Err(TalosError::ClientError(format!(
                    "app-server exited with {status}"
                )))
            }
        } else {
            Ok(())
        }
    }

    async fn request_typed<P, T>(&mut self, method: &str, params: P) -> TalosResult<T>
    where
        P: serde::Serialize,
        T: DeserializeOwned,
    {
        let value = self.request_value(method, params).await?;
        serde_json::from_value(value)
            .map_err(|error| TalosError::SerializationError(error.to_string()))
    }

    async fn request_value<P>(&mut self, method: &str, params: P) -> TalosResult<Value>
    where
        P: serde::Serialize,
    {
        let id = self.next_request_id();
        let params = serde_json::to_value(params)
            .map_err(|error| TalosError::SerializationError(error.to_string()))?;
        let mut request = JsonRpcRequest::new(RequestId::Number(id), method, Some(params));
        request.actor_credentials = self.actor_credentials_for_request(method);
        let (tx, rx) = oneshot::channel();
        self.pending_responses.lock().await.insert(id, tx);
        if let Err(error) = self.write_json(&request).await {
            self.pending_responses.lock().await.remove(&id);
            return Err(error);
        }
        let response = match tokio::time::timeout(APP_SERVER_REQUEST_TIMEOUT, rx).await {
            Ok(response) => response,
            Err(_) => {
                self.pending_responses.lock().await.remove(&id);
                return Err(TalosError::ClientError(
                    "app-server request timeout".to_string(),
                ));
            }
        };

        let result = response.map_err(|_| {
            TalosError::ClientError(format!(
                "app-server response channel closed for request {id}"
            ))
        })?;
        match result {
            Err(TalosError::AuthError(message))
                if self.use_cloud_session_cache
                    && self.actor_credentials.is_some()
                    && method != "auth.session.clear" =>
            {
                self.actor_credentials = None;
                Err(TalosError::AuthError(format!(
                    "{message}; cached cloud session was rejected, please sign in again"
                )))
            }
            other => other,
        }
    }

    fn actor_credentials_for_request(&self, method: &str) -> Option<ActorCredentials> {
        actor_credentials_for_method(method, &self.actor_credentials)
    }

    fn next_request_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    async fn write_json<T: serde::Serialize>(&mut self, value: &T) -> TalosResult<()> {
        let line = serde_json::to_string(value)
            .map_err(|error| TalosError::SerializationError(error.to_string()))?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| TalosError::ClientError("app-server client is shut down".to_string()))?;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| TalosError::ClientError(error.to_string()))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|error| TalosError::ClientError(error.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|error| TalosError::ClientError(error.to_string()))
    }
}

fn send_timeline_subscription_params(session_id: &str) -> SubscribeTimelineRequest {
    SubscribeTimelineRequest {
        session_id: session_id.to_string(),
        since_event_id: None,
        replay_limit: Some(256),
        close_when_idle: Some(false),
        close_on_terminal: Some(true),
    }
}

fn spawn_stdout_reader(
    stdout: ChildStdout,
    pending_responses: Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    subscription_senders: Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    events_tx: mpsc::Sender<TimelineEventEnvelope>,
    notifications_tx: mpsc::Sender<TimelineSubscriptionNotification>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            let read_result = reader.read_line(&mut line).await;
            let bytes = match read_result {
                Ok(bytes) => bytes,
                Err(error) => {
                    fail_pending_responses(&pending_responses, error.to_string()).await;
                    return;
                }
            };
            if bytes == 0 {
                fail_pending_responses(&pending_responses, "app-server stdout closed".to_string())
                    .await;
                return;
            }

            let message = match serde_json::from_str::<JsonRpcMessage>(line.trim_end()) {
                Ok(message) => message,
                Err(error) => {
                    fail_pending_responses(&pending_responses, error.to_string()).await;
                    return;
                }
            };

            match message {
                JsonRpcMessage::Response(response) => {
                    if let RequestId::Number(id) = response.id {
                        if let Some(sender) = pending_responses.lock().await.remove(&id) {
                            let _ = sender.send(Ok(response.result));
                        }
                    }
                }
                JsonRpcMessage::Error(error) => {
                    if let Some(RequestId::Number(id)) = error.id {
                        if let Some(sender) = pending_responses.lock().await.remove(&id) {
                            let _ = sender.send(Err(map_app_server_error(error.error)));
                        }
                    }
                }
                JsonRpcMessage::Notification(notification) => {
                    let Ok(Some(notification)) = decode_event_notification(notification) else {
                        continue;
                    };
                    let subscription_id = notification.subscription_id.clone();
                    let event = notification.event.clone();
                    dispatch_application_event(
                        &subscription_senders,
                        &subscription_id,
                        event.clone(),
                    )
                    .await;
                    match notifications_tx.try_send(notification) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            return;
                        }
                    }
                    match events_tx.try_send(event) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            return;
                        }
                    }
                }
                JsonRpcMessage::Request(_) => {}
            }
        }
    });
}

async fn fail_pending_responses(
    pending_responses: &Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    message: String,
) {
    let pending = {
        let mut guard = pending_responses.lock().await;
        std::mem::take(&mut *guard)
    };
    for (_, sender) in pending {
        let _ = sender.send(Err(TalosError::ClientError(message.clone())));
    }
}

async fn dispatch_application_event(
    subscription_senders: &Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    subscription_id: &str,
    event: TimelineEventEnvelope,
) {
    let terminal = is_terminal_application_event(&event);
    let mut sender = None;
    let mut should_remove = false;
    {
        let mut streams = subscription_senders.lock().await;
        let Some(stream) = streams.get_mut(subscription_id) else {
            return;
        };
        if event.session_id != stream.session_id {
            return;
        }
        match stream.turn_id.as_deref() {
            Some(turn_id) if event.turn_id == turn_id => {
                sender = Some(stream.sender.clone());
                should_remove = terminal;
            }
            Some(_) => {}
            None => {
                if stream.pending.len() < 256 {
                    stream.pending.push(event.clone());
                }
            }
        }
    }
    if let Some(sender) = sender {
        if sender.try_send(event).is_err() {
            should_remove = true;
        }
    }
    if should_remove {
        subscription_senders.lock().await.remove(subscription_id);
    }
}

fn validate_app_server_input(input: Vec<TalosInputItem>) -> TalosResult<Vec<TalosInputItem>> {
    for item in &input {
        match item {
            TalosInputItem::Text { .. } | TalosInputItem::SkillSelection { .. } => {}
            TalosInputItem::Image { .. }
            | TalosInputItem::File { .. }
            | TalosInputItem::Voice { .. }
            | TalosInputItem::Video { .. } => {
                return Err(TalosError::ValidationError(
                    "app-server protocol currently supports text and skill selection input items"
                        .to_string(),
                ));
            }
        }
    }
    Ok(input)
}

fn decode_event_notification(
    notification: JsonRpcNotification,
) -> TalosResult<Option<TimelineSubscriptionNotification>> {
    let Some(params) = notification.params else {
        return Ok(None);
    };
    if !notification.method.starts_with("timeline.") {
        return Ok(None);
    }
    serde_json::from_value(params)
        .map(Some)
        .map_err(|error| TalosError::SerializationError(error.to_string()))
}

fn map_app_server_error(error: talos_app_protocol::errors::AppServerErrorBody) -> TalosError {
    use talos_app_protocol::errors::{
        CONFLICT, FORBIDDEN, INVALID_PARAMS, NOT_FOUND, UNAUTHORIZED, VALIDATION_FAILED,
    };

    match error.code {
        UNAUTHORIZED => TalosError::AuthError(error.message),
        FORBIDDEN => TalosError::AuthorizationDenied,
        NOT_FOUND => TalosError::ResourceNotFound(error.message),
        CONFLICT => TalosError::Conflict(error.message),
        VALIDATION_FAILED | INVALID_PARAMS => TalosError::ValidationError(error.message),
        -32008 => TalosError::ResourceExpired(error.message),
        _ => TalosError::ClientError(error.message),
    }
}

fn actor_credentials_for_method(
    method: &str,
    credentials: &Option<ActorCredentials>,
) -> Option<ActorCredentials> {
    if matches!(
        method,
        "initialize"
            | "auth.login.start"
            | "auth.login.status"
            | "auth.login.cancel"
            | "auth.session.read"
            | "auth.session.clear"
    ) {
        None
    } else {
        credentials.clone()
    }
}

#[cfg(test)]
fn is_business_method(method: &str) -> bool {
    !matches!(
        method,
        "initialize"
            | "auth.login.start"
            | "auth.login.status"
            | "auth.login.cancel"
            | "auth.session.read"
            | "auth.session.store"
            | "auth.session.clear"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_types::talos::{
        TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineEventType,
        TalosTurnStartedEvent, TimelineEventEnvelope,
    };

    #[test]
    fn rejects_non_text_input() {
        let result = validate_app_server_input(vec![TalosInputItem::Image {
            url: "file://image.png".to_string(),
            mime_type: None,
        }]);

        assert!(matches!(result, Err(TalosError::ValidationError(_))));
    }

    #[test]
    fn accepts_skill_selection_input_without_protocol_mapping() {
        let result = validate_app_server_input(vec![TalosInputItem::SkillSelection {
            name: "review".to_string(),
            path: "/tmp/review/SKILL.md".to_string(),
        }])
        .expect("skill selection should be supported");

        assert_eq!(
            result,
            vec![TalosInputItem::SkillSelection {
                name: "review".to_string(),
                path: "/tmp/review/SKILL.md".to_string(),
            }]
        );
    }

    #[test]
    fn skips_cached_actor_credentials_for_local_session_methods() {
        let credentials = Some(ActorCredentials {
            credential: ActorCredential::ExistingSessionToken {
                token: "cloud-token".to_string(),
            },
        });

        assert_eq!(
            actor_credentials_for_method("initialize", &credentials),
            None
        );
        assert_eq!(
            actor_credentials_for_method("auth.session.read", &credentials),
            None
        );
        assert_eq!(
            actor_credentials_for_method("auth.session.clear", &credentials),
            None
        );
        assert_eq!(
            actor_credentials_for_method("auth.login.start", &credentials),
            None
        );
        assert_eq!(
            actor_credentials_for_method("auth.login.status", &credentials),
            None
        );
        assert_eq!(
            actor_credentials_for_method("auth.login.cancel", &credentials),
            None
        );
        assert_eq!(
            actor_credentials_for_method("session.list", &credentials),
            credentials
        );
    }

    #[test]
    fn touches_cached_session_only_for_business_methods() {
        assert!(!is_business_method("initialize"));
        assert!(!is_business_method("auth.session.read"));
        assert!(!is_business_method("auth.session.store"));
        assert!(!is_business_method("auth.session.clear"));
        assert!(!is_business_method("auth.login.start"));
        assert!(!is_business_method("auth.login.status"));
        assert!(!is_business_method("auth.login.cancel"));
        assert!(is_business_method("user.current"));
        assert!(is_business_method("session.list"));
    }

    #[test]
    fn send_timeline_events_keeps_pre_turn_subscription_open_while_idle() {
        let params = send_timeline_subscription_params("session-1");

        assert_eq!(params.session_id, "session-1");
        assert_eq!(params.close_when_idle, Some(false));
        assert_eq!(params.close_on_terminal, Some(true));
    }

    #[tokio::test]
    async fn dispatch_application_event_routes_only_matching_subscription() {
        let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
        let (first_tx, mut first_rx) = mpsc::channel(4);
        let (second_tx, mut second_rx) = mpsc::channel(4);
        {
            let mut streams = subscription_senders.lock().await;
            streams.insert(
                "sub-1".to_string(),
                SubscriptionStream {
                    session_id: "session-1".to_string(),
                    sender: first_tx,
                    turn_id: Some("turn-1".to_string()),
                    pending: Vec::new(),
                },
            );
            streams.insert(
                "sub-2".to_string(),
                SubscriptionStream {
                    session_id: "session-1".to_string(),
                    sender: second_tx,
                    turn_id: Some("turn-1".to_string()),
                    pending: Vec::new(),
                },
            );
        }

        dispatch_application_event(&subscription_senders, "sub-1", timeline_envelope("ev_1", 1))
            .await;

        assert_eq!(first_rx.recv().await.expect("first event").event_id, "ev_1");
        assert!(second_rx.try_recv().is_err());
    }

    fn timeline_envelope(event_id: &str, event_seq: i64) -> TimelineEventEnvelope {
        let event = TalosTimelineEvent {
            event_seq: Some(event_seq),
            event_id: Some(event_id.to_string()),
            created_at_ms: Some(event_seq),
            event_type: TalosTimelineEventType::TurnStarted,
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: event_seq as u64,
            payload: TalosTimelineEventPayload::TurnStarted(TalosTurnStartedEvent {
                request_id: "req-1".to_string(),
                turn_id: "turn-1".to_string(),
                started_at_ms: event_seq as u64,
            }),
        };
        TimelineEventEnvelope {
            event_id: event_id.to_string(),
            event_seq,
            created_at_ms: event_seq,
            session_id: event.session_id.clone(),
            turn_id: event.turn_id.clone(),
            message_id: event.message_id.clone(),
            event,
        }
    }
}
