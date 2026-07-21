use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use talos_app_protocol::events::{
    SubscriptionTerminatedEnvelope, TimelineSubscriptionNotification,
};
use talos_app_protocol::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, RequestId};
#[cfg(test)]
use talos_app_protocol::requests::ActorCredential;
use talos_app_protocol::requests::{
    ActorCredentials, ClientCapabilities, ClientInfo, InitializeParams, InitializeResponse,
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsRefreshRequest,
    SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, CloudLoginCancelResponse, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse, ConfigReloadRequest,
    ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse, EventUnsubscribeRequest,
    EventUnsubscribeResponse, FsDirectoryCreateRequest, FsDirectoryCreateResponse,
    FsDirectoryReadRequest, FsDirectoryReadResponse, InterruptTurnRequest, InterruptTurnResponse,
    MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest, MemoryFileReadResponse,
    MemoryFileWriteRequest, MemoryFileWriteResponse, MemoryGetRequest, MemoryGetResponse,
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
    SkillsReadResponse, StartTurnRequest, StartTurnResponse, SubmitDynamicToolResultRequest,
    SubmitDynamicToolResultResponse, SubscribeTimelineRequest, SubscribeTimelineResponse,
    TimelineRecoveryResponse, UserCurrentRequest, UserCurrentResponse, WorkspaceDocListRequest,
    WorkspaceDocListResponse, WorkspaceDocReadRequest, WorkspaceDocReadResponse,
    WorkspaceDocWriteRequest, WorkspaceDocWriteResponse, WorkspaceInfoRequest,
    WorkspaceInfoResponse,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::{GatewayClient, TimelineEventStream, TimelineStreamSubscription};
use talos_types::talos::TimelineEventEnvelope;

mod response;

#[path = "app_server/credentials.rs"]
mod credentials;

#[cfg(test)]
use credentials::is_business_method;
use credentials::{actor_credentials_for_method, existing_session_credentials};

#[derive(Debug, Clone)]
pub struct AppServerClientConfig {
    pub app_server_bin: String,
    pub client_name: String,
    pub client_version: String,
    pub actor_credentials: Option<ActorCredentials>,
    pub use_cloud_session_cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppServerOwnership {
    OwnedChild,
    ExternalTransport,
    BorrowedProcess,
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
    ownership: AppServerOwnership,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    next_id: i32,
    pending_responses: Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    subscription_senders: Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    pending_subscription_events: Arc<Mutex<HashMap<String, Vec<TimelineEventEnvelope>>>>,
    events: mpsc::Receiver<TimelineEventEnvelope>,
    notifications: mpsc::Receiver<TimelineSubscriptionNotification>,
    actor_credentials: Option<ActorCredentials>,
    use_cloud_session_cache: bool,
}

struct SubscriptionStream {
    session_id: String,
    sender: mpsc::Sender<TimelineEventEnvelope>,
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
        let pending_subscription_events = Arc::new(Mutex::new(HashMap::new()));
        let (events_tx, events_rx) = mpsc::channel(1024);
        let (notifications_tx, notifications_rx) = mpsc::channel(1024);
        spawn_stdout_reader(
            stdout,
            pending_responses.clone(),
            subscription_senders.clone(),
            pending_subscription_events.clone(),
            events_tx,
            notifications_tx,
        );
        Ok(Self {
            ownership: AppServerOwnership::OwnedChild,
            child: Some(child),
            stdin: Some(stdin),
            next_id: 1,
            pending_responses,
            subscription_senders,
            pending_subscription_events,
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

    pub(crate) async fn restore_cloud_session_candidate(&mut self) -> TalosResult<bool> {
        let response: AuthSessionReadResponse = self
            .request_typed("auth.session.read", AuthSessionReadRequest {})
            .await?;
        if let Some(session) = response.session {
            self.actor_credentials = Some(existing_session_credentials(session.access_token));
            return Ok(true);
        }
        Ok(false)
    }

    async fn apply_cloud_login_credentials(
        &mut self,
        response: &CloudLoginStatusResponse,
    ) -> TalosResult<()> {
        let CloudLoginStatusResponse::Authenticated { access_token, .. } = response else {
            return Ok(());
        };
        self.actor_credentials = Some(existing_session_credentials(access_token.clone()));
        Ok(())
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

    pub(crate) async fn shutdown(&mut self) -> TalosResult<()> {
        let stdin = self.stdin.take();
        let child = self.child.take();

        if let Some(mut stdin) = stdin {
            stdin
                .shutdown()
                .await
                .map_err(|error| TalosError::ClientError(error.to_string()))?;
        }
        if let (AppServerOwnership::OwnedChild, Some(mut child)) = (self.ownership, child) {
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

#[async_trait]
impl GatewayClient for AppServerClient {
    async fn read_auth_session(
        &mut self,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        self.request_typed("auth.session.read", request).await
    }

    async fn clear_auth_session(
        &mut self,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse> {
        let response = self.request_typed("auth.session.clear", request).await?;
        self.actor_credentials = None;
        Ok(response)
    }

    async fn auth_logout(&mut self, request: AuthLogoutRequest) -> TalosResult<AuthLogoutResponse> {
        let response = self.request_typed("auth.logout", request).await?;
        self.actor_credentials = None;
        Ok(response)
    }

    async fn start_cloud_login(
        &mut self,
        request: CloudLoginStartRequest,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.request_typed("auth.login.start", request).await
    }

    async fn cloud_login_status(
        &mut self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        let response = self.request_typed("auth.login.status", request).await?;
        self.apply_cloud_login_credentials(&response).await?;
        Ok(response)
    }

    async fn cancel_cloud_login(
        &mut self,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse> {
        self.request_typed("auth.login.cancel", request).await
    }

    async fn current_user(
        &mut self,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse> {
        self.request_typed("user.current", request).await
    }

    async fn read_profile(
        &mut self,
        request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse> {
        self.request_typed("profile/read", request).await
    }

    async fn list_models(&mut self, request: ModelListRequest) -> TalosResult<ModelListResponse> {
        self.request_typed("model.list", request).await
    }

    async fn get_config(&mut self, request: ConfigGetRequest) -> TalosResult<ConfigGetResponse> {
        self.request_typed("config.get", request).await
    }

    async fn update_config(
        &mut self,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigUpdateResponse> {
        self.request_typed("config.update", request).await
    }

    async fn delete_config(
        &mut self,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        self.request_typed("config.delete", request).await
    }

    async fn reload_config(
        &mut self,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        self.request_typed("config.reload", request).await
    }

    async fn list_skills(&mut self, request: SkillsListRequest) -> TalosResult<SkillsListResponse> {
        self.request_typed("skills.list", request).await
    }

    async fn refresh_skills(
        &mut self,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.request_typed("skills.refresh", request).await
    }

    async fn suggest_skills(
        &mut self,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        self.request_typed("skills.suggest", request).await
    }

    async fn read_skill(&mut self, request: SkillsReadRequest) -> TalosResult<SkillsReadResponse> {
        self.request_typed("skills.read", request).await
    }

    async fn resolve_skills(
        &mut self,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        self.request_typed("skills.resolve", request).await
    }

    async fn list_sessions(
        &mut self,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        self.request_typed("session.list", request).await
    }

    async fn get_session(&mut self, request: SessionGetRequest) -> TalosResult<SessionGetResponse> {
        self.request_typed("session.get", request).await
    }

    async fn get_session_history(
        &mut self,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        self.request_typed("session.history", request).await
    }

    async fn get_active_turn(
        &mut self,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        self.request_typed("session.active_turn", request).await
    }

    async fn patch_session(
        &mut self,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        self.request_typed("session.patch", request).await
    }

    async fn close_session(
        &mut self,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        self.request_typed("session.close", request).await
    }

    async fn delete_session(
        &mut self,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        self.request_typed("session.delete", request).await
    }

    async fn compact_session(
        &mut self,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse> {
        self.request_typed("session.compact", request).await
    }

    async fn clear_session(
        &mut self,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse> {
        self.request_typed("session.clear", request).await
    }

    async fn recover_timeline(
        &mut self,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse> {
        self.request_typed("timeline.recover", request).await
    }

    async fn subscribe_timeline(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse> {
        self.request_typed("event.subscribe", request).await
    }

    async fn open_timeline_stream(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineStreamSubscription> {
        let session_id = request.session_id.clone();
        let response = self.subscribe_timeline(request).await?;
        let subscription_id = response.subscription_id.clone();
        let (tx, rx) = mpsc::channel(256);
        {
            let mut streams = self.subscription_senders.lock().await;
            let buffered_events = self
                .pending_subscription_events
                .lock()
                .await
                .remove(&subscription_id)
                .unwrap_or_default();
            for event in buffered_events {
                if event.session_id == session_id && tx.try_send(event).is_err() {
                    break;
                }
            }
            streams.insert(
                subscription_id,
                SubscriptionStream {
                    session_id,
                    sender: tx,
                },
            );
        }
        Ok(TimelineStreamSubscription {
            response,
            stream: TimelineEventStream::new(rx),
        })
    }

    async fn unsubscribe_timeline(
        &mut self,
        request: EventUnsubscribeRequest,
    ) -> TalosResult<EventUnsubscribeResponse> {
        self.subscription_senders
            .lock()
            .await
            .remove(&request.subscription_id);
        self.pending_subscription_events
            .lock()
            .await
            .remove(&request.subscription_id);
        self.request_typed("event.unsubscribe", request).await
    }

    async fn start_turn(&mut self, request: StartTurnRequest) -> TalosResult<StartTurnResponse> {
        self.request_typed("turn.start", request).await
    }

    async fn retry_turn(&mut self, request: RetryTurnRequest) -> TalosResult<RetryTurnResponse> {
        self.request_typed("turn.retry", request).await
    }

    async fn interrupt_turn(
        &mut self,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.request_typed("turn.interrupt", request).await
    }

    async fn submit_dynamic_tool_result(
        &mut self,
        request: SubmitDynamicToolResultRequest,
    ) -> TalosResult<SubmitDynamicToolResultResponse> {
        self.request_typed("tool.dynamic_result.submit", request)
            .await
    }

    async fn list_projects(
        &mut self,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        self.request_typed("project.list", request).await
    }

    async fn get_project(
        &mut self,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        self.request_typed("project.read", request).await
    }

    async fn create_project(
        &mut self,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        self.request_typed("project.create", request).await
    }

    async fn patch_project(
        &mut self,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        self.request_typed("project.patch", request).await
    }

    async fn delete_project(
        &mut self,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        self.request_typed("project.delete", request).await
    }

    async fn read_fs_directory(
        &mut self,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse> {
        self.request_typed("fs.directory.read", request).await
    }

    async fn create_fs_directory(
        &mut self,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        self.request_typed("fs.directory.create", request).await
    }

    async fn list_plugin_sources(
        &mut self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.request_typed("plugin.source.list", request).await
    }

    async fn add_plugin_source(
        &mut self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.request_typed("plugin.source.add", request).await
    }

    async fn refresh_plugin_source(
        &mut self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.request_typed("plugin.source.refresh", request).await
    }

    async fn remove_plugin_source(
        &mut self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.request_typed("plugin.source.remove", request).await
    }

    async fn search_plugins(
        &mut self,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        self.request_typed("plugin.search", request).await
    }

    async fn list_plugins(
        &mut self,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        self.request_typed("plugin.list", request).await
    }

    async fn read_plugin(&mut self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        self.request_typed("plugin.read", request).await
    }

    async fn install_plugin(
        &mut self,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        self.request_typed("plugin.install", request).await
    }

    async fn update_plugin(
        &mut self,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        self.request_typed("plugin.update", request).await
    }

    async fn uninstall_plugin(
        &mut self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.request_typed("plugin.uninstall", request).await
    }

    async fn enable_plugin(
        &mut self,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        self.request_typed("plugin.enable", request).await
    }

    async fn disable_plugin(
        &mut self,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        self.request_typed("plugin.disable", request).await
    }

    async fn list_memory_files(
        &mut self,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        self.request_typed("memory.file.list", request).await
    }

    async fn read_memory_file(
        &mut self,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        self.request_typed("memory.file.read", request).await
    }

    async fn write_memory_file(
        &mut self,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        self.request_typed("memory.file.write", request).await
    }

    async fn search_memory(
        &mut self,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        self.request_typed("memory/search", request).await
    }

    async fn get_memory(&mut self, request: MemoryGetRequest) -> TalosResult<MemoryGetResponse> {
        self.request_typed("memory/get", request).await
    }

    async fn list_workspace_docs(
        &mut self,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        self.request_typed("workspace.doc.list", request).await
    }

    async fn read_workspace_doc(
        &mut self,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        self.request_typed("workspace.doc.read", request).await
    }

    async fn write_workspace_doc(
        &mut self,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        self.request_typed("workspace.doc.write", request).await
    }

    async fn workspace_info(
        &mut self,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        self.request_typed("workspace.info", request).await
    }

    async fn shutdown(&mut self) -> TalosResult<()> {
        self.shutdown().await
    }
}

fn decode_subscription_terminated(
    notification: &JsonRpcNotification,
) -> TalosResult<Option<SubscriptionTerminatedEnvelope>> {
    if notification.method != "subscription.terminated" {
        return Ok(None);
    }
    let Some(params) = notification.params.clone() else {
        return Ok(None);
    };
    serde_json::from_value(params)
        .map(Some)
        .map_err(|error| TalosError::SerializationError(error.to_string()))
}

fn spawn_stdout_reader(
    stdout: ChildStdout,
    pending_responses: Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    subscription_senders: Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    pending_subscription_events: Arc<Mutex<HashMap<String, Vec<TimelineEventEnvelope>>>>,
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
                    response::fail_pending_responses(&pending_responses, error.to_string()).await;
                    return;
                }
            };
            if bytes == 0 {
                response::fail_pending_responses(
                    &pending_responses,
                    "app-server stdout closed".to_string(),
                )
                .await;
                return;
            }

            let message = match serde_json::from_str::<JsonRpcMessage>(line.trim_end()) {
                Ok(message) => message,
                Err(error) => {
                    response::fail_pending_responses(&pending_responses, error.to_string()).await;
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
                    if let Ok(Some(terminated)) = decode_subscription_terminated(&notification) {
                        subscription_senders
                            .lock()
                            .await
                            .remove(&terminated.subscription_id);
                        pending_subscription_events
                            .lock()
                            .await
                            .remove(&terminated.subscription_id);
                        continue;
                    }
                    let Ok(Some(notification)) = decode_event_notification(notification) else {
                        continue;
                    };
                    let subscription_id = notification.subscription_id.clone();
                    let event = notification.event.clone();
                    dispatch_application_event(
                        &subscription_senders,
                        &pending_subscription_events,
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

async fn dispatch_application_event(
    subscription_senders: &Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    pending_subscription_events: &Arc<Mutex<HashMap<String, Vec<TimelineEventEnvelope>>>>,
    subscription_id: &str,
    event: TimelineEventEnvelope,
) {
    let sender = {
        let streams = subscription_senders.lock().await;
        let Some(stream) = streams.get(subscription_id) else {
            pending_subscription_events
                .lock()
                .await
                .entry(subscription_id.to_string())
                .or_default()
                .push(event);
            return;
        };
        if event.session_id != stream.session_id {
            return;
        }
        stream.sender.clone()
    };
    if sender.try_send(event).is_err() {
        subscription_senders.lock().await.remove(subscription_id);
    }
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
        ACTIVE_TURN_RUNNING, CANCELLED, CONFLICT, FORBIDDEN, INVALID_PARAMS, NOT_FOUND,
        REQUEST_TIMEOUT, RESOURCE_EXPIRED, RUNTIME_UNAVAILABLE, TRANSPORT_UNAVAILABLE,
        UNAUTHORIZED, VALIDATION_FAILED,
    };

    match error.code {
        UNAUTHORIZED => TalosError::AuthError(error.message),
        FORBIDDEN => TalosError::AuthorizationDenied,
        NOT_FOUND => TalosError::ResourceNotFound(error.message),
        ACTIVE_TURN_RUNNING => TalosError::ActiveTurnRunning(error.message),
        CONFLICT => TalosError::Conflict(error.message),
        VALIDATION_FAILED | INVALID_PARAMS => TalosError::ValidationError(error.message),
        RESOURCE_EXPIRED => TalosError::ResourceExpired(error.message),
        RUNTIME_UNAVAILABLE => TalosError::RuntimeUnavailable(error.message),
        TRANSPORT_UNAVAILABLE => TalosError::TransportUnavailable(error.message),
        REQUEST_TIMEOUT => TalosError::Timeout(error.message),
        CANCELLED => TalosError::Cancelled(error.message),
        _ => TalosError::ClientError(error.message),
    }
}

#[cfg(test)]
#[path = "app_server_tests.rs"]
mod tests;
