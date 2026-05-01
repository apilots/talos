use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;
use talos_app_protocol::events::ApplicationEventEnvelope;
use talos_app_protocol::jsonrpc::{JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, RequestId};
use talos_app_protocol::requests::{
    ActorCredential, ActorCredentials, AdminProjectListParams, AdminProjectListResponse,
    AdminSessionListParams, AdminSessionListResponse, ClientCapabilities, ClientInfo,
    ConfigReadParams, EventSubscribeParams, EventSubscribeResponse, InitializeParams,
    InitializeResponse, MemoryFileListParams as ProtocolMemoryFileListParams, MemoryFileReadParams,
    MemoryFileWriteParams, ModelListParams, ProjectCreateParams, ProjectDeleteParams,
    SessionCloseParams,
    ProjectListParams, ProjectPatchParams, ProjectReadParams, ProtocolInputItem,
    SessionCreateParams, SessionDeleteParams, SessionListParams, SessionPatchParams,
    SessionReadParams, SessionTurnsParams, ToolApprovalSubmitParams, ToolDynamicResultSubmitParams,
    TurnInterruptParams, TurnStartParams, TurnStartResponse, WorkspaceDocListParams,
    WorkspaceDocReadParams, WorkspaceDocWriteParams,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_error::{TalosError, TalosResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::app_server_events::{is_terminal_application_event, map_application_event};
use crate::common::{expected_file_version_token, workspace_doc_kind_token};
use crate::ExpectedFileVersion;
use crate::{
    MemoryFileDetail, MemoryFileSummary, ProjectPatch, ProjectRecord, TalosEvent,
    TalosMessageResponse, WorkspaceDocDetail, WorkspaceDocKind, WorkspaceDocSummary,
};
use crate::{ModelInfo, TalosInputItem, TalosMessageRequest, TalosSession};

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
                .unwrap_or_else(|_| "talos".to_string()),
            launch_token,
            client_name: "talos-rust-sdk".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }
}

pub struct AppServerClient {
    child: Child,
    stdin: ChildStdin,
    next_id: i32,
    pending_responses: Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    subscription_senders: Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    events: mpsc::Receiver<ApplicationEventEnvelope>,
}

struct SubscriptionStream {
    sender: mpsc::Sender<TalosEvent>,
    turn_id: Option<String>,
    pending: Vec<ApplicationEventEnvelope>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ListResponse<T> {
    data: Vec<T>,
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
            .arg("app-server")
            .env("TALOS_LOCAL_LAUNCH_TOKEN", &config.launch_token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                TalosError::ClientError(format!(
                    "spawn {} app-server: {error}",
                    config.app_server_bin
                ))
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
        spawn_stdout_reader(
            stdout,
            pending_responses.clone(),
            subscription_senders.clone(),
            events_tx,
        );
        Ok(Self {
            child,
            stdin,
            next_id: 1,
            pending_responses,
            subscription_senders,
            events: events_rx,
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

    pub async fn get_session(&mut self, session_id: &str) -> TalosResult<TalosSession> {
        self.request_typed(
            "session.read",
            SessionReadParams {
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

    pub async fn get_session_turns(
        &mut self,
        session_id: &str,
    ) -> TalosResult<Vec<TalosMessageResponse>> {
        self.request_typed::<_, ListResponse<TalosMessageResponse>>(
            "session.turns",
            SessionTurnsParams {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|response| response.data)
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
    ) -> TalosResult<TurnStartResponse> {
        self.request_typed(
            "turn.start",
            TurnStartParams {
                session_id: session_id.to_string(),
                input: map_input(request.input)?,
                workspace_id: request.workspace_id,
                client_request_id,
            },
        )
        .await
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

    pub async fn submit_tool_approval(
        &mut self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        let _: Value = self
            .request_typed(
                "tool.approval.submit",
                ToolApprovalSubmitParams {
                    session_id: session_id.to_string(),
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    approved,
                    reason,
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

    pub async fn next_event(&mut self) -> TalosResult<ApplicationEventEnvelope> {
        self.events
            .recv()
            .await
            .ok_or_else(|| TalosError::ClientError("app-server event stream closed".to_string()))
    }

    pub async fn next_talos_event(&mut self) -> TalosResult<TalosEvent> {
        let event = self.next_event().await?;
        map_application_event(event)
    }

    pub async fn send_message_events(
        &mut self,
        session_id: &str,
        request: TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>> {
        let subscription = self
            .subscribe_events(EventSubscribeParams {
                session_id: session_id.to_string(),
                since_event_id: None,
                replay_limit: Some(256),
            })
            .await?;
        let subscription_id = subscription.subscription_id;
        let (tx, rx) = mpsc::channel(256);
        self.subscription_senders.lock().await.insert(
            subscription_id.clone(),
            SubscriptionStream {
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
                if event.turn_id.as_deref() != Some(turn_id.as_str()) {
                    continue;
                }
                terminal_seen |= is_terminal_application_event(&event);
                if let Ok(talos_event) = map_application_event(event) {
                    outbound.push((stream.sender.clone(), talos_event));
                }
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

    pub async fn shutdown(mut self) -> TalosResult<()> {
        self.stdin
            .shutdown()
            .await
            .map_err(|error| TalosError::ClientError(error.to_string()))?;
        drop(self.stdin);
        let status = self
            .child
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
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| TalosError::ClientError(error.to_string()))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|error| TalosError::ClientError(error.to_string()))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| TalosError::ClientError(error.to_string()))
    }
}

fn spawn_stdout_reader(
    stdout: ChildStdout,
    pending_responses: Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    subscription_senders: Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    events_tx: mpsc::Sender<ApplicationEventEnvelope>,
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
                    let Ok(Some(event)) = decode_event_notification(notification) else {
                        continue;
                    };
                    dispatch_application_event(&subscription_senders, event.clone()).await;
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
    event: ApplicationEventEnvelope,
) {
    let subscription_id = event.subscription_id.clone();
    let terminal = is_terminal_application_event(&event);
    let mut outbound = None;
    let mut should_remove = false;
    {
        let mut streams = subscription_senders.lock().await;
        if let Some(stream) = streams.get_mut(&subscription_id) {
            match stream.turn_id.as_deref() {
                Some(turn_id) if event.turn_id.as_deref() == Some(turn_id) => {
                    if let Ok(talos_event) = map_application_event(event) {
                        outbound = Some((stream.sender.clone(), talos_event));
                    }
                    should_remove = terminal;
                }
                Some(_) => {}
                None => {
                    if stream.pending.len() < 256 {
                        stream.pending.push(event);
                    }
                }
            }
        }
    }
    if let Some((sender, talos_event)) = outbound {
        if sender.try_send(talos_event).is_err() {
            should_remove = true;
        }
    }
    if should_remove {
        subscription_senders.lock().await.remove(&subscription_id);
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

fn decode_event_notification(
    notification: JsonRpcNotification,
) -> TalosResult<Option<ApplicationEventEnvelope>> {
    let Some(params) = notification.params else {
        return Ok(None);
    };
    if params.get("event").is_none() {
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

    #[test]
    fn rejects_non_text_input() {
        let result = map_input(vec![TalosInputItem::Image {
            url: "file://image.png".to_string(),
            mime_type: None,
        }]);

        assert!(matches!(result, Err(TalosError::ValidationError(_))));
    }
}
