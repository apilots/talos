use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use talos_app_protocol::events::TimelineSubscriptionNotification;
use talos_app_protocol::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, RequestId};
use talos_app_protocol::requests::{
    ActorCredential, ActorCredentials, AdminProjectListParams, AdminProjectListResponse,
    AdminSessionListParams, AdminSessionListResponse, ClientCapabilities, ClientInfo,
    ConfigReadParams, ConfigValueWriteParams, EventSubscribeParams, EventSubscribeResponse,
    EventUnsubscribeParams, EventUnsubscribeResponse, InitializeParams, InitializeResponse,
    MemoryFileListParams as ProtocolMemoryFileListParams, MemoryFileReadParams,
    MemoryFileWriteParams, MessageCommandStatus as ProtocolMessageCommandStatus, ModelListParams,
    ProjectCreateParams, ProjectDeleteParams, ProjectListParams, ProjectPatchParams,
    ProjectReadParams, ProtocolInputItem, SessionActiveTurnParams, SessionCloseParams,
    SessionCreateParams, SessionDeleteParams, SessionHistoryParams, SessionListParams,
    SessionPatchParams, SessionReadParams, TimelineActiveTurnSnapshot, TimelineRecoveryParams,
    TimelineRecoverySnapshot, ToolDynamicResultSubmitParams, TurnInterruptParams, TurnStartParams,
    TurnStartResponse, WorkspaceDocListParams, WorkspaceDocReadParams, WorkspaceDocWriteParams,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_auth::{AdminCreateUserRequest, AdminPatchUserRequest, UserDto};
use talos_config::{LlmConfig, LlmConfigRead};
use talos_error::{TalosError, TalosResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::app_server_events::is_terminal_application_event;
use crate::common::{expected_file_version_token, workspace_doc_kind_token};
use crate::ExpectedFileVersion;
use crate::{
    MemoryFileDetail, MemoryFileSummary, MessageCommandAck, MessageCommandStatus, ProjectPatch,
    ProjectRecord, WorkspaceDocDetail, WorkspaceDocKind, WorkspaceDocSummary,
};
use crate::{ModelInfo, TalosInputItem, TalosMessageRequest, TalosSession};
use talos_types::talos::{SessionHistorySnapshot, TimelineEventEnvelope};

#[derive(Debug, Clone)]
pub struct AppServerClientConfig {
    pub app_server_bin: String,
    pub launch_token: String,
    pub client_name: String,
    pub client_version: String,
}

impl AppServerClientConfig {
    pub fn from_env() -> TalosResult<Self> {
        let launch_token = std::env::var("TALOS_LOCAL_LAUNCH_TOKEN").map_err(|_| {
            TalosError::InvalidConfiguration(
                "TALOS_LOCAL_LAUNCH_TOKEN is required for app-server client".to_string(),
            )
        })?;
        Ok(Self {
            app_server_bin: std::env::var("TALOS_APP_SERVER_BIN")
                .unwrap_or_else(|_| "app-server".to_string()),
            launch_token,
            client_name: "talos-rust-sdk".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
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
}

struct SubscriptionStream {
    session_id: String,
    sender: mpsc::Sender<TimelineEventEnvelope>,
    turn_id: Option<String>,
    pending: Vec<TimelineEventEnvelope>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ListResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Clone, Serialize)]
struct EmptyParams {}

#[derive(Debug, Clone, Serialize)]
struct AdminUserPatchRequest {
    user_id: String,
    username: Option<String>,
    email: Option<String>,
    password: Option<String>,
    role: Option<talos_auth::UserRole>,
    disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct AdminUserDeleteRequest {
    user_id: String,
}

const APP_SERVER_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl AppServerClient {
    pub async fn connect(config: AppServerClientConfig) -> TalosResult<Self> {
        let mut client = Self::spawn(&config).await?;
        client.initialize(&config).await?;
        client.notify_initialized().await?;
        Ok(client)
    }

    async fn spawn(config: &AppServerClientConfig) -> TalosResult<Self> {
        let mut child = Command::new(&config.app_server_bin)
            .env("TALOS_LOCAL_LAUNCH_TOKEN", &config.launch_token)
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
                actor_credentials: ActorCredentials {
                    credential: ActorCredential::LocalLaunchToken {
                        token: config.launch_token.clone(),
                    },
                },
            },
        )
        .await
    }

    async fn notify_initialized(&mut self) -> TalosResult<()> {
        let notification = JsonRpcNotification::new("initialized", None);
        self.write_json(&notification).await
    }

    pub async fn list_models(&mut self) -> TalosResult<Vec<ModelInfo>> {
        self.request_typed::<_, ListResponse<ModelInfo>>("model.list", ModelListParams {})
            .await
            .map(|response| response.data)
    }

    pub async fn config_read(&mut self) -> TalosResult<Value> {
        self.request_typed("config.read", ConfigReadParams {}).await
    }

    pub async fn get_llm_config(&mut self) -> TalosResult<LlmConfigRead> {
        self.request_typed("config.read", ConfigReadParams {}).await
    }

    pub async fn update_llm_config(&mut self, config: LlmConfig) -> TalosResult<LlmConfigRead> {
        let value = serde_json::to_string(&config)
            .map_err(|error| TalosError::SerializationError(error.to_string()))?;
        self.request_typed(
            "config.value.write",
            ConfigValueWriteParams {
                path: "llm".to_string(),
                value,
            },
        )
        .await
    }

    pub async fn list_memory_files(&mut self) -> TalosResult<Vec<MemoryFileSummary>> {
        self.request_typed::<_, ListResponse<MemoryFileSummary>>(
            "memory.file.list",
            ProtocolMemoryFileListParams { limit: Some(32) },
        )
        .await
        .map(|response| response.data)
    }

    pub async fn read_memory_file(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> TalosResult<MemoryFileDetail> {
        self.request_typed(
            "memory.file.read",
            MemoryFileReadParams { path: path.into() },
        )
        .await
    }

    pub async fn write_memory_file(
        &mut self,
        path: impl Into<PathBuf>,
        content: String,
        expected_version: Option<String>,
    ) -> TalosResult<MemoryFileDetail> {
        self.request_typed(
            "memory.file.write",
            MemoryFileWriteParams {
                path: path.into(),
                content,
                expected_version,
            },
        )
        .await
    }

    pub async fn list_workspace_docs(&mut self) -> TalosResult<Vec<WorkspaceDocSummary>> {
        self.request_typed::<_, ListResponse<WorkspaceDocSummary>>(
            "workspace.doc.list",
            WorkspaceDocListParams {},
        )
        .await
        .map(|response| response.data)
    }

    pub async fn read_workspace_doc(
        &mut self,
        kind: WorkspaceDocKind,
    ) -> TalosResult<WorkspaceDocDetail> {
        self.request_typed(
            "workspace.doc.read",
            WorkspaceDocReadParams {
                kind: workspace_doc_kind_token(kind).to_string(),
            },
        )
        .await
    }

    pub async fn write_workspace_doc(
        &mut self,
        kind: WorkspaceDocKind,
        content: String,
        expected_version: ExpectedFileVersion,
    ) -> TalosResult<WorkspaceDocDetail> {
        self.request_typed(
            "workspace.doc.write",
            WorkspaceDocWriteParams {
                kind: workspace_doc_kind_token(kind).to_string(),
                content,
                expected_version: expected_file_version_token(expected_version),
            },
        )
        .await
    }

    pub async fn create_session(
        &mut self,
        params: SessionCreateParams,
    ) -> TalosResult<TalosSession> {
        self.request_typed("session.create", params).await
    }

    pub async fn list_sessions(
        &mut self,
        params: SessionListParams,
    ) -> TalosResult<Vec<TalosSession>> {
        self.request_typed::<_, ListResponse<TalosSession>>("session.list", params)
            .await
            .map(|response| response.data)
    }

    pub async fn list_admin_sessions(
        &mut self,
        params: AdminSessionListParams,
    ) -> TalosResult<AdminSessionListResponse> {
        self.request_typed("admin.session.list", params).await
    }

    pub async fn list_admin_users(&mut self) -> TalosResult<Vec<UserDto>> {
        self.request_typed::<_, ListResponse<UserDto>>("admin.user.list", EmptyParams {})
            .await
            .map(|response| response.data)
    }

    pub async fn create_admin_user(
        &mut self,
        request: AdminCreateUserRequest,
    ) -> TalosResult<UserDto> {
        self.request_typed("admin.user.create", request).await
    }

    pub async fn patch_admin_user(
        &mut self,
        user_id: &str,
        request: AdminPatchUserRequest,
    ) -> TalosResult<UserDto> {
        self.request_typed(
            "admin.user.patch",
            AdminUserPatchRequest {
                user_id: user_id.to_string(),
                username: request.username,
                email: request.email,
                password: request.password,
                role: request.role,
                disabled: request.disabled,
            },
        )
        .await
    }

    pub async fn delete_admin_user(&mut self, user_id: &str) -> TalosResult<()> {
        let _: Value = self
            .request_typed(
                "admin.user.delete",
                AdminUserDeleteRequest {
                    user_id: user_id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn get_session(&mut self, session_id: &str) -> TalosResult<TalosSession> {
        self.request_typed(
            "session.read",
            SessionReadParams {
                session_id: session_id.to_string(),
            },
        )
        .await
    }

    pub async fn get_session_history(
        &mut self,
        session_id: &str,
    ) -> TalosResult<SessionHistorySnapshot> {
        self.request_typed(
            "session.history",
            SessionHistoryParams {
                session_id: session_id.to_string(),
            },
        )
        .await
    }

    pub async fn get_active_turn(
        &mut self,
        session_id: &str,
    ) -> TalosResult<TimelineActiveTurnSnapshot> {
        self.request_typed(
            "session.active_turn",
            SessionActiveTurnParams {
                session_id: session_id.to_string(),
            },
        )
        .await
    }

    pub async fn delete_session(&mut self, session_id: &str) -> TalosResult<()> {
        let _: Value = self
            .request_typed(
                "session.delete",
                SessionDeleteParams {
                    session_id: session_id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn close_session(&mut self, session_id: &str) -> TalosResult<()> {
        let _: Value = self
            .request_typed(
                "session.close",
                SessionCloseParams {
                    session_id: session_id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn recover_session(
        &mut self,
        session_id: &str,
    ) -> TalosResult<TimelineRecoverySnapshot> {
        self.recover_session_after(session_id, None).await
    }

    pub async fn recover_session_after(
        &mut self,
        session_id: &str,
        after_event_id: Option<String>,
    ) -> TalosResult<TimelineRecoverySnapshot> {
        self.request_typed(
            "session.recover",
            TimelineRecoveryParams {
                session_id: session_id.to_string(),
                after_event_id,
                replay_limit: None,
            },
        )
        .await
    }

    pub async fn list_projects(&mut self) -> TalosResult<Vec<ProjectRecord>> {
        self.request_typed::<_, ListResponse<ProjectRecord>>("project.list", ProjectListParams {})
            .await
            .map(|response| response.data)
    }

    pub async fn list_admin_projects(&mut self) -> TalosResult<Vec<ProjectRecord>> {
        self.request_typed::<_, AdminProjectListResponse>(
            "admin.project.list",
            AdminProjectListParams {},
        )
        .await
        .map(|response| response.data)
    }

    pub async fn get_project(&mut self, project_id: &str) -> TalosResult<ProjectRecord> {
        self.request_typed(
            "project.read",
            ProjectReadParams {
                project_id: project_id.to_string(),
            },
        )
        .await
    }

    pub async fn create_project(
        &mut self,
        name: impl Into<String>,
        directory: impl Into<PathBuf>,
    ) -> TalosResult<ProjectRecord> {
        self.request_typed(
            "project.create",
            ProjectCreateParams {
                name: name.into(),
                directory: directory.into(),
            },
        )
        .await
    }

    pub async fn patch_project(
        &mut self,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<ProjectRecord> {
        self.request_typed(
            "project.patch",
            ProjectPatchParams {
                project_id: project_id.to_string(),
                name: patch.name,
            },
        )
        .await
    }

    pub async fn update_project_name(
        &mut self,
        project_id: &str,
        name: impl Into<String>,
    ) -> TalosResult<ProjectRecord> {
        self.patch_project(
            project_id,
            ProjectPatch {
                name: Some(name.into()),
            },
        )
        .await
    }

    pub async fn delete_project(&mut self, project_id: &str) -> TalosResult<()> {
        let _: Value = self
            .request_typed(
                "project.delete",
                ProjectDeleteParams {
                    project_id: project_id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn update_session_model(
        &mut self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession> {
        let _: Value = self
            .request_typed(
                "session.patch",
                SessionPatchParams {
                    session_id: session_id.to_string(),
                    project_id: None,
                    current_directory: None,
                    model: Some(model.to_string()),
                },
            )
            .await?;
        self.get_session(session_id).await
    }

    pub async fn start_turn(
        &mut self,
        session_id: &str,
        request: TalosMessageRequest,
        client_request_id: Option<String>,
    ) -> TalosResult<MessageCommandAck> {
        let response: TurnStartResponse = self
            .request_typed(
                "turn.start",
                TurnStartParams {
                    session_id: session_id.to_string(),
                    input: map_input(request.input)?,
                    workspace_id: request.workspace_id,
                    client_request_id,
                },
            )
            .await?;
        Ok(map_turn_start_response(response))
    }

    pub async fn interrupt_turn(&mut self, session_id: &str) -> TalosResult<()> {
        let _: Value = self
            .request_typed(
                "turn.interrupt",
                TurnInterruptParams {
                    session_id: session_id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn submit_dynamic_tool_result(
        &mut self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let _: Value = self
            .request_typed(
                "tool.dynamic_result.submit",
                ToolDynamicResultSubmitParams {
                    session_id: session_id.to_string(),
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    output,
                    success,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn subscribe_events(
        &mut self,
        params: EventSubscribeParams,
    ) -> TalosResult<EventSubscribeResponse> {
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
            EventUnsubscribeParams {
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
        session_id: &str,
        request: TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let subscription = self
            .subscribe_events(send_timeline_subscription_params(session_id))
            .await?;
        let subscription_id = subscription.subscription_id;
        let (tx, rx) = mpsc::channel(256);
        self.subscription_senders.lock().await.insert(
            subscription_id.clone(),
            SubscriptionStream {
                session_id: session_id.to_string(),
                sender: tx,
                turn_id: None,
                pending: Vec::new(),
            },
        );
        let start_response = match self
            .start_turn(
                session_id,
                request,
                Some(format!("app-server-sdk-{}", uuid::Uuid::new_v4().simple())),
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.subscription_senders
                    .lock()
                    .await
                    .remove(&subscription_id);
                return Err(error);
            }
        };
        self.activate_subscription_stream(&subscription_id, start_response.turn_id)
            .await?;
        Ok(rx)
    }

    async fn activate_subscription_stream(
        &self,
        subscription_id: &str,
        turn_id: String,
    ) -> TalosResult<()> {
        let outbound = {
            let mut streams = self.subscription_senders.lock().await;
            let Some(stream) = streams.get_mut(subscription_id) else {
                return Ok(());
            };
            stream.turn_id = Some(turn_id.clone());
            let pending = std::mem::take(&mut stream.pending);
            let mut terminal_seen = false;
            let mut outbound = Vec::new();
            for event in pending {
                if event.turn_id != turn_id {
                    continue;
                }
                terminal_seen |= is_terminal_application_event(&event);
                outbound.push((stream.sender.clone(), event));
            }
            if terminal_seen {
                streams.remove(subscription_id);
            }
            outbound
        };

        for (sender, talos_event) in outbound {
            if sender.try_send(talos_event).is_err() {
                return Ok(());
            }
        }
        Ok(())
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
        let request = JsonRpcRequest::new(RequestId::Number(id), method, Some(params));
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

        response.map_err(|_| {
            TalosError::ClientError(format!(
                "app-server response channel closed for request {id}"
            ))
        })?
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

fn send_timeline_subscription_params(session_id: &str) -> EventSubscribeParams {
    EventSubscribeParams {
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

fn map_input(input: Vec<TalosInputItem>) -> TalosResult<Vec<ProtocolInputItem>> {
    input
        .into_iter()
        .map(|item| match item {
            TalosInputItem::Text { text } => Ok(ProtocolInputItem::Text { text }),
            _ => Err(TalosError::ValidationError(
                "app-server protocol currently supports text input items".to_string(),
            )),
        })
        .collect()
}

fn map_turn_start_response(response: TurnStartResponse) -> MessageCommandAck {
    MessageCommandAck {
        request_id: response.request_id,
        message_id: response.message_id,
        session_id: response.session_id,
        turn_id: response.turn_id,
        status: map_message_command_status(response.status),
    }
}

fn map_message_command_status(status: ProtocolMessageCommandStatus) -> MessageCommandStatus {
    match status {
        ProtocolMessageCommandStatus::Pending => MessageCommandStatus::Pending,
        ProtocolMessageCommandStatus::Accepted => MessageCommandStatus::Accepted,
        ProtocolMessageCommandStatus::Failed => MessageCommandStatus::Failed,
        ProtocolMessageCommandStatus::FailedNoFirstEvent => {
            MessageCommandStatus::FailedNoFirstEvent
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use talos_types::talos::{
        TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineEventType,
        TalosTurnStartedEvent, TimelineEventEnvelope,
    };

    #[test]
    fn rejects_non_text_input() {
        let result = map_input(vec![TalosInputItem::Image {
            url: "file://image.png".to_string(),
            mime_type: None,
        }]);

        assert!(matches!(result, Err(TalosError::ValidationError(_))));
    }

    #[test]
    fn maps_turn_start_response_to_message_ack() {
        let ack = map_turn_start_response(TurnStartResponse {
            request_id: "req-1".to_string(),
            message_id: Some("msg-1".to_string()),
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: ProtocolMessageCommandStatus::Accepted,
        });

        assert_eq!(ack.request_id, "req-1");
        assert_eq!(ack.message_id.as_deref(), Some("msg-1"));
        assert_eq!(ack.status, MessageCommandStatus::Accepted);
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
