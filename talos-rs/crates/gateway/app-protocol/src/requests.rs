use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use ts_rs::TS;

use crate::errors::AppServerErrorBody;
use crate::version::ProtocolVersion;
use talos_agent_api::{InterruptTurnRequest, SubmitTurnInputRequest};
use talos_types::{
    AuthLogoutRequest, AuthSessionClearRequest, AuthSessionReadRequest, ConfigDeleteRequest,
    ConfigGetRequest, ConfigReloadRequest, ConfigUpdateRequest, EventUnsubscribeRequest,
    FileDeleteRequest, FileListRequest, FileReadRequest, FileUpdateRequest, FileUploadRequest,
    FsDirectoryCreateRequest, FsDirectoryReadRequest, MemoryFileListRequest, MemoryFileReadRequest,
    MemoryFileWriteRequest, MemoryGetRequest, MemorySearchRequest, MessageQueueAddRequest,
    MessageQueueDeleteRequest, MessageQueueListRequest, MessageQueueReorderRequest,
    MessageQueueStartRequest, MessageQueueUpdateRequest, ModelListRequest, PluginDisableRequest,
    PluginEnableRequest, PluginInstallRequest, PluginListRequest, PluginReadRequest,
    PluginSearchRequest, PluginSourceAddRequest, PluginSourceListRequest,
    PluginSourceRefreshRequest, PluginSourceRemoveRequest, PluginUninstallRequest,
    PluginUpdateRequest, ProfileReadRequest, ProjectCreateRequest, ProjectDeleteRequest,
    ProjectListRequest, ProjectPatchRequest, ProjectReadRequest, RecoverTimelineRequest,
    RetryTurnRequest, SessionActiveTurnRequest, SessionClearRequest, SessionCloseRequest,
    SessionCompactRequest, SessionDeleteRequest, SessionGetRequest, SessionHistoryRequest,
    SessionListRequest, SessionPatchRequest, SkillsListRequest, SkillsReadRequest,
    SkillsRefreshRequest, SkillsResolveRequest, SkillsSuggestRequest, StartTurnRequest,
    SubscribeTimelineRequest, UserCurrentRequest, WorkspaceDocListRequest, WorkspaceDocReadRequest,
    WorkspaceDocWriteRequest, WorkspaceInfoRequest,
};

#[path = "client_method.rs"]
mod client_method;
pub use client_method::ClientMethod;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ClientRequestParams {
    #[serde(rename = "initialize")]
    Initialize(InitializeParams),
    #[serde(rename = "desktop/connection/read")]
    DesktopConnectionRead(Option<()>),
    #[serde(rename = "desktop/connection/set")]
    DesktopConnectionSet(talos_types::DesktopConnectionSetParams),
    #[serde(rename = "desktop/workerIdentity")]
    DesktopWorkerIdentity(Option<()>),
    #[serde(rename = "desktop/workerEnroll")]
    DesktopWorkerEnroll(talos_types::DesktopWorkerEnrollParams),
    #[serde(rename = "computer/list")]
    ComputerList(talos_types::ComputerListRequest),
    #[serde(rename = "computer/read")]
    ComputerRead(talos_types::GetNodeRequest),
    #[serde(rename = "computer/create")]
    ComputerCreate(talos_types::CreateNodeRequest),
    #[serde(rename = "computer/rename")]
    ComputerRename(talos_types::RenameComputerRequest),
    #[serde(rename = "computer/setStatus")]
    ComputerSetStatus(talos_types::SetComputerStatusRequest),
    #[serde(rename = "computer/rotateCredential")]
    ComputerRotateCredential(talos_types::RotateCredentialRequest),
    #[serde(rename = "profile/read")]
    ProfileRead(ProfileReadRequest),
    #[serde(rename = "session/list")]
    SessionList(SessionListRequest),
    #[serde(rename = "session/get")]
    SessionGet(SessionGetRequest),
    #[serde(rename = "session/patch")]
    SessionPatch(SessionPatchRequest),
    #[serde(rename = "session/delete")]
    SessionDelete(SessionDeleteRequest),
    #[serde(rename = "session/close")]
    SessionClose(SessionCloseRequest),
    #[serde(rename = "session/compact")]
    SessionCompact(SessionCompactRequest),
    #[serde(rename = "session/clear")]
    SessionClear(SessionClearRequest),
    #[serde(rename = "session/active_turn")]
    SessionActiveTurn(SessionActiveTurnRequest),
    #[serde(rename = "session/history")]
    SessionHistory(SessionHistoryRequest),
    #[serde(rename = "timeline/recover")]
    TimelineRecover(RecoverTimelineRequest),
    #[serde(rename = "project/create")]
    ProjectCreate(ProjectCreateRequest),
    #[serde(rename = "project/list")]
    ProjectList(ProjectListRequest),
    #[serde(rename = "project/read")]
    ProjectRead(ProjectReadRequest),
    #[serde(rename = "project/patch")]
    ProjectPatch(ProjectPatchRequest),
    #[serde(rename = "project/delete")]
    ProjectDelete(ProjectDeleteRequest),
    #[serde(rename = "fs/directory/read")]
    FsDirectoryRead(FsDirectoryReadRequest),
    #[serde(rename = "fs/directory/create")]
    FsDirectoryCreate(FsDirectoryCreateRequest),
    #[serde(rename = "turn/start")]
    TurnStart(StartTurnRequest),
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt(InterruptTurnRequest),
    #[serde(rename = "turn/input")]
    TurnInput(SubmitTurnInputRequest),
    #[serde(rename = "turn/retry")]
    TurnRetry(RetryTurnRequest),
    #[serde(rename = "messageQueue/add")]
    MessageQueueAdd(MessageQueueAddRequest),
    #[serde(rename = "messageQueue/list")]
    MessageQueueList(MessageQueueListRequest),
    #[serde(rename = "messageQueue/update")]
    MessageQueueUpdate(MessageQueueUpdateRequest),
    #[serde(rename = "messageQueue/delete")]
    MessageQueueDelete(MessageQueueDeleteRequest),
    #[serde(rename = "messageQueue/reorder")]
    MessageQueueReorder(MessageQueueReorderRequest),
    #[serde(rename = "messageQueue/start")]
    MessageQueueStart(MessageQueueStartRequest),
    #[serde(rename = "event/subscribe")]
    EventSubscribe(SubscribeTimelineRequest),
    #[serde(rename = "event/unsubscribe")]
    EventUnsubscribe(EventUnsubscribeRequest),
    #[serde(rename = "model/list")]
    ModelList(ModelListRequest),
    #[serde(rename = "config/get")]
    ConfigGet(ConfigGetRequest),
    #[serde(rename = "config/update")]
    ConfigUpdate(ConfigUpdateRequest),
    #[serde(rename = "config/delete")]
    ConfigDelete(ConfigDeleteRequest),
    #[serde(rename = "config/reload")]
    ConfigReload(ConfigReloadRequest),
    #[serde(rename = "file/upload")]
    FileUpload(FileUploadParams),
    #[serde(rename = "file/list")]
    FileList(FileListRequest),
    #[serde(rename = "file/read")]
    FileRead(FileReadRequest),
    #[serde(rename = "file/content")]
    FileContent(FileReadRequest),
    #[serde(rename = "file/update")]
    FileUpdate(FileUpdateRequest),
    #[serde(rename = "file/delete")]
    FileDelete(FileDeleteRequest),
    #[serde(rename = "memory/file/list")]
    MemoryFileList(MemoryFileListRequest),
    #[serde(rename = "memory/file/read")]
    MemoryFileRead(MemoryFileReadRequest),
    #[serde(rename = "memory/file/write")]
    MemoryFileWrite(MemoryFileWriteRequest),
    #[serde(rename = "workspace/info")]
    WorkspaceInfo(WorkspaceInfoRequest),
    #[serde(rename = "workspace/doc/list")]
    WorkspaceDocList(WorkspaceDocListRequest),
    #[serde(rename = "workspace/doc/read")]
    WorkspaceDocRead(WorkspaceDocReadRequest),
    #[serde(rename = "workspace/doc/write")]
    WorkspaceDocWrite(WorkspaceDocWriteRequest),
    #[serde(rename = "skills/list")]
    SkillsList(SkillsListRequest),
    #[serde(rename = "skills/refresh")]
    SkillsRefresh(SkillsRefreshRequest),
    #[serde(rename = "skills/suggest")]
    SkillsSuggest(SkillsSuggestRequest),
    #[serde(rename = "skills/read")]
    SkillsRead(SkillsReadRequest),
    #[serde(rename = "skills/resolve")]
    SkillsResolve(SkillsResolveRequest),
    #[serde(rename = "plugin/source/list")]
    PluginSourceList(PluginSourceListRequest),
    #[serde(rename = "plugin/source/add")]
    PluginSourceAdd(PluginSourceAddRequest),
    #[serde(rename = "plugin/source/refresh")]
    PluginSourceRefresh(PluginSourceRefreshRequest),
    #[serde(rename = "plugin/source/remove")]
    PluginSourceRemove(PluginSourceRemoveRequest),
    #[serde(rename = "plugin/search")]
    PluginSearch(PluginSearchRequest),
    #[serde(rename = "mcp/server/list")]
    McpServerList(talos_types::McpServerListRequest),
    #[serde(rename = "mcp/server/credentials/update")]
    McpCredentialsUpdate(talos_types::McpCredentialsUpdateRequest),
    #[serde(rename = "mcp/server/auth/start")]
    McpAuthenticationStart(talos_types::McpAuthenticationStartRequest),
    #[serde(rename = "mcp/server/auth/read")]
    McpAuthenticationRead(talos_types::McpAuthenticationReadRequest),
    #[serde(rename = "mcp/server/import")]
    McpServerImport(talos_types::McpServerImportRequest),
    #[serde(rename = "plugin/list")]
    PluginList(PluginListRequest),
    #[serde(rename = "plugin/read")]
    PluginRead(PluginReadRequest),
    #[serde(rename = "plugin/install")]
    PluginInstall(PluginInstallRequest),
    #[serde(rename = "plugin/update")]
    PluginUpdate(PluginUpdateRequest),
    #[serde(rename = "plugin/uninstall")]
    PluginUninstall(PluginUninstallRequest),
    #[serde(rename = "plugin/enable")]
    PluginEnable(PluginEnableRequest),
    #[serde(rename = "plugin/disable")]
    PluginDisable(PluginDisableRequest),
    #[serde(rename = "auth/logout")]
    AuthLogout(AuthLogoutRequest),
    #[serde(rename = "auth/login/start")]
    AuthLoginStart(talos_auth::CloudLoginStartRequest),
    #[serde(rename = "auth/login/status")]
    AuthLoginStatus(talos_auth::CloudLoginStatusRequest),
    #[serde(rename = "auth/login/cancel")]
    AuthLoginCancel(talos_auth::CloudLoginCancelRequest),
    #[serde(rename = "auth/session/read")]
    AuthSessionRead(AuthSessionReadRequest),
    #[serde(rename = "auth/session/clear")]
    AuthSessionClear(AuthSessionClearRequest),
    #[serde(rename = "user/current")]
    UserCurrent(UserCurrentRequest),
    #[serde(rename = "memory/search")]
    MemorySearch(MemorySearchRequest),
    #[serde(rename = "memory/get")]
    MemoryGet(MemoryGetRequest),
}

/// App Server upload envelope; other file methods use shared request objects directly.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileUploadParams {
    pub request: FileUploadRequest,
    pub data_base64: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientRequest {
    pub method: ClientMethod,
    pub params: ClientRequestParams,
}

impl ClientRequest {
    pub fn decode(
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<Self, AppServerErrorBody> {
        let method_kind = ClientMethod::from_str(method)?;
        let params_value = params.unwrap_or_else(|| match method_kind {
            ClientMethod::DesktopConnectionRead | ClientMethod::DesktopWorkerIdentity => {
                serde_json::Value::Null
            }
            _ => serde_json::Value::Object(Default::default()),
        });
        let params = match method_kind {
            ClientMethod::Initialize => ClientRequestParams::Initialize(decode(params_value)?),
            ClientMethod::DesktopConnectionRead => {
                ClientRequestParams::DesktopConnectionRead(decode(params_value)?)
            }
            ClientMethod::DesktopConnectionSet => {
                ClientRequestParams::DesktopConnectionSet(decode(params_value)?)
            }
            ClientMethod::DesktopWorkerIdentity => {
                ClientRequestParams::DesktopWorkerIdentity(decode(params_value)?)
            }
            ClientMethod::DesktopWorkerEnroll => {
                ClientRequestParams::DesktopWorkerEnroll(decode(params_value)?)
            }
            ClientMethod::ComputerList => ClientRequestParams::ComputerList(decode(params_value)?),
            ClientMethod::ComputerRead => ClientRequestParams::ComputerRead(decode(params_value)?),
            ClientMethod::ComputerCreate => {
                ClientRequestParams::ComputerCreate(decode(params_value)?)
            }
            ClientMethod::ComputerRename => {
                ClientRequestParams::ComputerRename(decode(params_value)?)
            }
            ClientMethod::ComputerSetStatus => {
                ClientRequestParams::ComputerSetStatus(decode(params_value)?)
            }
            ClientMethod::ComputerRotateCredential => {
                ClientRequestParams::ComputerRotateCredential(decode(params_value)?)
            }
            ClientMethod::ProfileRead => ClientRequestParams::ProfileRead(decode(params_value)?),
            ClientMethod::SessionList => ClientRequestParams::SessionList(decode(params_value)?),
            ClientMethod::SessionGet => ClientRequestParams::SessionGet(decode(params_value)?),
            ClientMethod::SessionPatch => ClientRequestParams::SessionPatch(decode(params_value)?),
            ClientMethod::SessionDelete => {
                ClientRequestParams::SessionDelete(decode(params_value)?)
            }
            ClientMethod::SessionClose => ClientRequestParams::SessionClose(decode(params_value)?),
            ClientMethod::SessionCompact => {
                ClientRequestParams::SessionCompact(decode(params_value)?)
            }
            ClientMethod::SessionClear => ClientRequestParams::SessionClear(decode(params_value)?),
            ClientMethod::SessionActiveTurn => {
                ClientRequestParams::SessionActiveTurn(decode(params_value)?)
            }
            ClientMethod::SessionHistory => {
                ClientRequestParams::SessionHistory(decode(params_value)?)
            }
            ClientMethod::TimelineRecover => {
                ClientRequestParams::TimelineRecover(decode(params_value)?)
            }
            ClientMethod::ProjectCreate => {
                ClientRequestParams::ProjectCreate(decode(params_value)?)
            }
            ClientMethod::ProjectList => ClientRequestParams::ProjectList(decode(params_value)?),
            ClientMethod::ProjectRead => ClientRequestParams::ProjectRead(decode(params_value)?),
            ClientMethod::ProjectPatch => ClientRequestParams::ProjectPatch(decode(params_value)?),
            ClientMethod::ProjectDelete => {
                ClientRequestParams::ProjectDelete(decode(params_value)?)
            }
            ClientMethod::FsDirectoryRead => {
                ClientRequestParams::FsDirectoryRead(decode(params_value)?)
            }
            ClientMethod::FsDirectoryCreate => {
                ClientRequestParams::FsDirectoryCreate(decode(params_value)?)
            }
            ClientMethod::TurnStart => ClientRequestParams::TurnStart(decode(params_value)?),
            ClientMethod::TurnInterrupt => {
                ClientRequestParams::TurnInterrupt(decode(params_value)?)
            }
            ClientMethod::TurnInput => ClientRequestParams::TurnInput(decode(params_value)?),
            ClientMethod::TurnRetry => ClientRequestParams::TurnRetry(decode(params_value)?),
            ClientMethod::MessageQueueAdd => {
                ClientRequestParams::MessageQueueAdd(decode(params_value)?)
            }
            ClientMethod::MessageQueueList => {
                ClientRequestParams::MessageQueueList(decode(params_value)?)
            }
            ClientMethod::MessageQueueUpdate => {
                ClientRequestParams::MessageQueueUpdate(decode(params_value)?)
            }
            ClientMethod::MessageQueueDelete => {
                ClientRequestParams::MessageQueueDelete(decode(params_value)?)
            }
            ClientMethod::MessageQueueReorder => {
                ClientRequestParams::MessageQueueReorder(decode(params_value)?)
            }
            ClientMethod::MessageQueueStart => {
                ClientRequestParams::MessageQueueStart(decode(params_value)?)
            }
            ClientMethod::EventSubscribe => {
                ClientRequestParams::EventSubscribe(decode(params_value)?)
            }
            ClientMethod::EventUnsubscribe => {
                ClientRequestParams::EventUnsubscribe(decode(params_value)?)
            }
            ClientMethod::ModelList => ClientRequestParams::ModelList(decode(params_value)?),
            ClientMethod::ConfigGet => ClientRequestParams::ConfigGet(decode(params_value)?),
            ClientMethod::ConfigUpdate => ClientRequestParams::ConfigUpdate(decode(params_value)?),
            ClientMethod::ConfigDelete => ClientRequestParams::ConfigDelete(decode(params_value)?),
            ClientMethod::ConfigReload => ClientRequestParams::ConfigReload(decode(params_value)?),
            ClientMethod::FileUpload => ClientRequestParams::FileUpload(decode(params_value)?),
            ClientMethod::FileList => ClientRequestParams::FileList(decode(params_value)?),
            ClientMethod::FileRead => ClientRequestParams::FileRead(decode(params_value)?),
            ClientMethod::FileContent => ClientRequestParams::FileContent(decode(params_value)?),
            ClientMethod::FileUpdate => ClientRequestParams::FileUpdate(decode(params_value)?),
            ClientMethod::FileDelete => ClientRequestParams::FileDelete(decode(params_value)?),
            ClientMethod::MemoryFileList => {
                ClientRequestParams::MemoryFileList(decode(params_value)?)
            }
            ClientMethod::MemoryFileRead => {
                ClientRequestParams::MemoryFileRead(decode(params_value)?)
            }
            ClientMethod::MemoryFileWrite => {
                ClientRequestParams::MemoryFileWrite(decode(params_value)?)
            }
            ClientMethod::WorkspaceInfo => {
                ClientRequestParams::WorkspaceInfo(decode(params_value)?)
            }
            ClientMethod::WorkspaceDocList => {
                ClientRequestParams::WorkspaceDocList(decode(params_value)?)
            }
            ClientMethod::WorkspaceDocRead => {
                ClientRequestParams::WorkspaceDocRead(decode(params_value)?)
            }
            ClientMethod::WorkspaceDocWrite => {
                ClientRequestParams::WorkspaceDocWrite(decode(params_value)?)
            }
            ClientMethod::SkillsList => ClientRequestParams::SkillsList(decode(params_value)?),
            ClientMethod::SkillsRefresh => {
                ClientRequestParams::SkillsRefresh(decode(params_value)?)
            }
            ClientMethod::SkillsSuggest => {
                ClientRequestParams::SkillsSuggest(decode(params_value)?)
            }
            ClientMethod::SkillsRead => ClientRequestParams::SkillsRead(decode(params_value)?),
            ClientMethod::SkillsResolve => {
                ClientRequestParams::SkillsResolve(decode(params_value)?)
            }
            ClientMethod::PluginSourceList => {
                ClientRequestParams::PluginSourceList(decode(params_value)?)
            }
            ClientMethod::PluginSourceAdd => {
                ClientRequestParams::PluginSourceAdd(decode(params_value)?)
            }
            ClientMethod::PluginSourceRefresh => {
                ClientRequestParams::PluginSourceRefresh(decode(params_value)?)
            }
            ClientMethod::PluginSourceRemove => {
                ClientRequestParams::PluginSourceRemove(decode(params_value)?)
            }
            ClientMethod::PluginSearch => ClientRequestParams::PluginSearch(decode(params_value)?),
            ClientMethod::McpServerImport => {
                ClientRequestParams::McpServerImport(decode(params_value)?)
            }
            ClientMethod::McpAuthenticationStart => {
                ClientRequestParams::McpAuthenticationStart(decode(params_value)?)
            }
            ClientMethod::McpAuthenticationRead => {
                ClientRequestParams::McpAuthenticationRead(decode(params_value)?)
            }
            ClientMethod::McpCredentialsUpdate => {
                ClientRequestParams::McpCredentialsUpdate(decode(params_value)?)
            }
            ClientMethod::McpServerList => {
                ClientRequestParams::McpServerList(decode(params_value)?)
            }
            ClientMethod::PluginList => ClientRequestParams::PluginList(decode(params_value)?),
            ClientMethod::PluginRead => ClientRequestParams::PluginRead(decode(params_value)?),
            ClientMethod::PluginInstall => {
                ClientRequestParams::PluginInstall(decode(params_value)?)
            }
            ClientMethod::PluginUpdate => ClientRequestParams::PluginUpdate(decode(params_value)?),
            ClientMethod::PluginUninstall => {
                ClientRequestParams::PluginUninstall(decode(params_value)?)
            }
            ClientMethod::PluginEnable => ClientRequestParams::PluginEnable(decode(params_value)?),
            ClientMethod::PluginDisable => {
                ClientRequestParams::PluginDisable(decode(params_value)?)
            }
            ClientMethod::AuthLogout => ClientRequestParams::AuthLogout(decode(params_value)?),
            ClientMethod::AuthLoginStart => {
                ClientRequestParams::AuthLoginStart(decode(params_value)?)
            }
            ClientMethod::AuthLoginStatus => {
                ClientRequestParams::AuthLoginStatus(decode(params_value)?)
            }
            ClientMethod::AuthLoginCancel => {
                ClientRequestParams::AuthLoginCancel(decode(params_value)?)
            }
            ClientMethod::AuthSessionRead => {
                ClientRequestParams::AuthSessionRead(decode(params_value)?)
            }
            ClientMethod::AuthSessionClear => {
                ClientRequestParams::AuthSessionClear(decode(params_value)?)
            }
            ClientMethod::UserCurrent => ClientRequestParams::UserCurrent(decode(params_value)?),
            ClientMethod::MemorySearch => ClientRequestParams::MemorySearch(decode(params_value)?),
            ClientMethod::MemoryGet => ClientRequestParams::MemoryGet(decode(params_value)?),
        };
        Ok(Self {
            method: method_kind,
            params,
        })
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: serde_json::Value) -> Result<T, AppServerErrorBody> {
    serde_json::from_value(value)
        .map_err(|error| AppServerErrorBody::invalid_params(error.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct InitializeParams {
    pub client_info: ClientInfo,
    pub capabilities: ClientCapabilities,
    #[serde(default)]
    pub actor_credentials: Option<ActorCredentials>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct InitializeResponse {
    pub protocol_version: ProtocolVersion,
    pub server_info: ServerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ClientInfo {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    pub version: String,
    pub kind: ClientKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClientKind {
    Desktop,
    Cli,
    Sdk,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub auth_enabled: bool,
    pub cloud_auth_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ClientCapabilities {
    pub protocol_version: ProtocolVersion,
    pub supports_server_requests: bool,
    pub supports_event_replay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ActorCredentials {
    pub credential: ActorCredential,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActorCredential {
    ExistingSessionToken { token: String },
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
