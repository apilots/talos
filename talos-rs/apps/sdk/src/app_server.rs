use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use talos_agent_api::{
    InterruptTurnRequest, InterruptTurnResponse, RecoverTimelineResponse,
    SessionActiveTurnResponse, SessionHistoryResponse, SubmitTurnInputRequest,
    SubmitTurnInputResponse, TimelineEvent,
};
use talos_app_protocol::events::TimelineEventNotification;
use talos_app_protocol::jsonrpc::{JsonRpcMessage, JsonRpcNotification};
#[cfg(test)]
use talos_app_protocol::requests::ActorCredential;
use talos_app_protocol::requests::{
    ActorCredentials, ClientCapabilities, ClientInfo, ClientKind, InitializeParams,
    InitializeResponse,
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
    ConfigDeleteResponse, ConfigGetRequest, ConfigReloadRequest, ConfigReloadResponse,
    ConfigResponse, ConfigUpdateRequest, EventUnsubscribeRequest, EventUnsubscribeResponse,
    FsDirectoryCreateRequest, FsDirectoryCreateResponse, FsDirectoryReadRequest,
    FsDirectoryReadResponse, MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest,
    MemoryFileReadResponse, MemoryFileWriteRequest, MemoryFileWriteResponse, MemoryGetRequest,
    MemoryGetResponse, MemorySearchRequest, MemorySearchResponse, MessageQueue,
    MessageQueueAddRequest, MessageQueueDeleteRequest, MessageQueueListRequest,
    MessageQueueReorderRequest, MessageQueueStartRequest, MessageQueueUpdateRequest,
    ModelListRequest, ModelListResponse, PluginDisableRequest, PluginDisableResponse,
    PluginEnableRequest, PluginEnableResponse, PluginInstallRequest, PluginInstallResponse,
    PluginListRequest, PluginListResponse, PluginReadRequest, PluginReadResponse,
    PluginSearchRequest, PluginSearchResponse, PluginSourceAddRequest, PluginSourceAddResponse,
    PluginSourceListRequest, PluginSourceListResponse, PluginSourceRefreshRequest,
    PluginSourceRefreshResponse, PluginSourceRemoveRequest, PluginSourceRemoveResponse,
    PluginUninstallRequest, PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
    ProfileReadRequest, ProfileReadResponse, ProjectCreateRequest, ProjectCreateResponse,
    ProjectDeleteRequest, ProjectDeleteResponse, ProjectListRequest, ProjectListResponse,
    ProjectPatchRequest, ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse,
    RecoverTimelineRequest, RetryTurnRequest, SessionActiveTurnRequest, SessionClearRequest,
    SessionClearResponse, SessionCloseRequest, SessionCloseResponse, SessionCompactRequest,
    SessionCompactResponse, SessionDeleteRequest, SessionDeleteResponse, SessionGetRequest,
    SessionGetResponse, SessionHistoryRequest, SessionListRequest, SessionListResponse,
    SessionPatchRequest, SessionUpdateResponse, SkillsListRequest, SkillsListResponse,
    SkillsReadRequest, SkillsReadResponse, SkillsRefreshRequest, SkillsResolveRequest,
    SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse, StartTurnRequest,
    StartTurnResponse, SubscribeTimelineRequest, SubscribeTimelineResponse, UserCurrentRequest,
    UserCurrentResponse, WorkspaceDocListRequest, WorkspaceDocListResponse,
    WorkspaceDocReadRequest, WorkspaceDocReadResponse, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse, WorkspaceInfoRequest, WorkspaceInfoResponse,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

mod events;
use events::{dispatch_message_queue_change, dispatch_timeline_event};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::{
    GatewayClient, MessageQueueChangeStream, TimelineEventStream, TimelineStreamSubscription,
};

mod request;
mod response;
mod stdout;
use stdout::spawn_stdout_reader;

#[path = "app_server/credentials.rs"]
mod credentials;
#[path = "app_server/files.rs"]
mod files;

use credentials::existing_session_credentials;
#[cfg(test)]
use credentials::is_business_method;

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
    pending_subscription_events: Arc<Mutex<HashMap<String, Vec<TimelineEvent>>>>,
    events: mpsc::Receiver<TimelineEvent>,
    notifications: mpsc::Receiver<TimelineEventNotification>,
    actor_credentials: Option<ActorCredentials>,
    use_cloud_session_cache: bool,
}

struct SubscriptionStream {
    session_id: String,
    sender: mpsc::Sender<TimelineEvent>,
    message_queue_sender: mpsc::Sender<talos_types::MessageQueueChangedNotification>,
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
                let diagnostic = format!("spawn {}: {error}", config.app_server_bin);
                TalosError::internal_with_source(
                    "Failed to start app-server; technical details were withheld.",
                    error,
                )
                .context(diagnostic)
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TalosError::internal("app-server stdin is not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TalosError::internal("app-server stdout is not piped"))?;
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
        let response: InitializeResponse = self
            .request_typed(
                "initialize",
                InitializeParams {
                    client_info: ClientInfo {
                        name: config.client_name.clone(),
                        title: None,
                        version: config.client_version.clone(),
                        kind: ClientKind::Sdk,
                    },
                    capabilities: ClientCapabilities {
                        protocol_version: ProtocolVersion::CURRENT,
                        supports_server_requests: false,
                        supports_event_replay: true,
                    },
                    actor_credentials: None,
                },
            )
            .await?;
        if response.protocol_version != ProtocolVersion::CURRENT {
            return Err(TalosError::internal(
                "Unsupported app-server protocol version; update the backend and client together.",
            ));
        }
        Ok(response)
    }

    async fn notify_initialized(&mut self) -> TalosResult<()> {
        let notification = JsonRpcNotification::new("initialized", None);
        self.write_json(&notification).await
    }

    pub(crate) async fn restore_cloud_session_candidate(&mut self) -> TalosResult<bool> {
        let response: AuthSessionReadResponse = self
            .request_typed("auth/session/read", AuthSessionReadRequest {})
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

    pub async fn next_event(&mut self) -> TalosResult<TimelineEvent> {
        self.events
            .recv()
            .await
            .ok_or_else(|| TalosError::internal("app-server event stream closed"))
    }

    pub async fn next_notification(&mut self) -> TalosResult<TimelineEventNotification> {
        self.notifications
            .recv()
            .await
            .ok_or_else(|| TalosError::internal("app-server notification stream closed"))
    }

    pub(crate) async fn shutdown(&mut self) -> TalosResult<()> {
        let stdin = self.stdin.take();
        let child = self.child.take();

        if let Some(mut stdin) = stdin {
            stdin.shutdown().await.map_err(|error| {
                TalosError::internal_with_source(
                    "App-server communication failed; technical details were withheld.",
                    error,
                )
            })?;
        }
        if let (AppServerOwnership::OwnedChild, Some(mut child)) = (self.ownership, child) {
            let status = child.wait().await.map_err(|error| {
                TalosError::internal_with_source(
                    "App-server communication failed; technical details were withheld.",
                    error,
                )
            })?;
            if status.success() {
                Ok(())
            } else {
                Err(TalosError::internal(
                    "app-server exited with; technical details were withheld.",
                )
                .context(format!("app-server exited with {status}")))
            }
        } else {
            Ok(())
        }
    }
}

#[path = "app_server/gateway.rs"]
mod gateway;

#[cfg(test)]
#[path = "app_server_tests.rs"]
mod tests;
