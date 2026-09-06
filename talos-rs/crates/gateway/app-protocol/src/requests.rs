use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
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
    WorkspaceDocWriteRequest,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub enum ClientMethod {
    #[serde(rename = "initialize")]
    Initialize,
    #[serde(rename = "profile/read")]
    ProfileRead,
    #[serde(rename = "session/list")]
    SessionList,
    #[serde(rename = "session/get")]
    SessionGet,
    #[serde(rename = "session/patch")]
    SessionPatch,
    #[serde(rename = "session/delete")]
    SessionDelete,
    #[serde(rename = "session/close")]
    SessionClose,
    #[serde(rename = "session/compact")]
    SessionCompact,
    #[serde(rename = "session/clear")]
    SessionClear,
    #[serde(rename = "session/active_turn")]
    SessionActiveTurn,
    #[serde(rename = "session/history")]
    SessionHistory,
    #[serde(rename = "timeline/recover")]
    TimelineRecover,
    #[serde(rename = "project/create")]
    ProjectCreate,
    #[serde(rename = "project/list")]
    ProjectList,
    #[serde(rename = "project/read")]
    ProjectRead,
    #[serde(rename = "project/patch")]
    ProjectPatch,
    #[serde(rename = "project/delete")]
    ProjectDelete,
    #[serde(rename = "fs/directory/read")]
    FsDirectoryRead,
    #[serde(rename = "fs/directory/create")]
    FsDirectoryCreate,
    #[serde(rename = "turn/start")]
    TurnStart,
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt,
    #[serde(rename = "turn/input")]
    TurnInput,
    #[serde(rename = "turn/retry")]
    TurnRetry,
    #[serde(rename = "messageQueue/add")]
    MessageQueueAdd,
    #[serde(rename = "messageQueue/list")]
    MessageQueueList,
    #[serde(rename = "messageQueue/update")]
    MessageQueueUpdate,
    #[serde(rename = "messageQueue/delete")]
    MessageQueueDelete,
    #[serde(rename = "messageQueue/reorder")]
    MessageQueueReorder,
    #[serde(rename = "messageQueue/start")]
    MessageQueueStart,
    #[serde(rename = "event/subscribe")]
    EventSubscribe,
    #[serde(rename = "event/unsubscribe")]
    EventUnsubscribe,
    #[serde(rename = "model/list")]
    ModelList,
    #[serde(rename = "config/get")]
    ConfigGet,
    #[serde(rename = "config/update")]
    ConfigUpdate,
    #[serde(rename = "config/delete")]
    ConfigDelete,
    #[serde(rename = "config/reload")]
    ConfigReload,
    #[serde(rename = "file/upload")]
    FileUpload,
    #[serde(rename = "file/list")]
    FileList,
    #[serde(rename = "file/read")]
    FileRead,
    #[serde(rename = "file/update")]
    FileUpdate,
    #[serde(rename = "file/delete")]
    FileDelete,
    #[serde(rename = "memory/file/list")]
    MemoryFileList,
    #[serde(rename = "memory/file/read")]
    MemoryFileRead,
    #[serde(rename = "memory/file/write")]
    MemoryFileWrite,
    #[serde(rename = "workspace/doc/list")]
    WorkspaceDocList,
    #[serde(rename = "workspace/doc/read")]
    WorkspaceDocRead,
    #[serde(rename = "workspace/doc/write")]
    WorkspaceDocWrite,
    #[serde(rename = "skills/list")]
    SkillsList,
    #[serde(rename = "skills/refresh")]
    SkillsRefresh,
    #[serde(rename = "skills/suggest")]
    SkillsSuggest,
    #[serde(rename = "skills/read")]
    SkillsRead,
    #[serde(rename = "skills/resolve")]
    SkillsResolve,
    #[serde(rename = "plugin/source/list")]
    PluginSourceList,
    #[serde(rename = "plugin/source/add")]
    PluginSourceAdd,
    #[serde(rename = "plugin/source/refresh")]
    PluginSourceRefresh,
    #[serde(rename = "plugin/source/remove")]
    PluginSourceRemove,
    #[serde(rename = "plugin/search")]
    PluginSearch,
    #[serde(rename = "plugin/list")]
    PluginList,
    #[serde(rename = "plugin/read")]
    PluginRead,
    #[serde(rename = "plugin/install")]
    PluginInstall,
    #[serde(rename = "plugin/update")]
    PluginUpdate,
    #[serde(rename = "plugin/uninstall")]
    PluginUninstall,
    #[serde(rename = "plugin/enable")]
    PluginEnable,
    #[serde(rename = "plugin/disable")]
    PluginDisable,
    #[serde(rename = "auth/logout")]
    AuthLogout,
    #[serde(rename = "auth/login/start")]
    AuthLoginStart,
    #[serde(rename = "auth/login/status")]
    AuthLoginStatus,
    #[serde(rename = "auth/login/cancel")]
    AuthLoginCancel,
    #[serde(rename = "auth/session/read")]
    AuthSessionRead,
    #[serde(rename = "auth/session/clear")]
    AuthSessionClear,
    #[serde(rename = "user/current")]
    UserCurrent,
    #[serde(rename = "memory/search")]
    MemorySearch,
    #[serde(rename = "memory/get")]
    MemoryGet,
}

impl ClientMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::ProfileRead => "profile/read",
            Self::SessionList => "session/list",
            Self::SessionGet => "session/get",
            Self::SessionPatch => "session/patch",
            Self::SessionDelete => "session/delete",
            Self::SessionClose => "session/close",
            Self::SessionCompact => "session/compact",
            Self::SessionClear => "session/clear",
            Self::SessionActiveTurn => "session/active_turn",
            Self::SessionHistory => "session/history",
            Self::TimelineRecover => "timeline/recover",
            Self::ProjectCreate => "project/create",
            Self::ProjectList => "project/list",
            Self::ProjectRead => "project/read",
            Self::ProjectPatch => "project/patch",
            Self::ProjectDelete => "project/delete",
            Self::FsDirectoryRead => "fs/directory/read",
            Self::FsDirectoryCreate => "fs/directory/create",
            Self::TurnStart => "turn/start",
            Self::TurnInterrupt => "turn/interrupt",
            Self::TurnInput => "turn/input",
            Self::TurnRetry => "turn/retry",
            Self::MessageQueueAdd => "messageQueue/add",
            Self::MessageQueueList => "messageQueue/list",
            Self::MessageQueueUpdate => "messageQueue/update",
            Self::MessageQueueDelete => "messageQueue/delete",
            Self::MessageQueueReorder => "messageQueue/reorder",
            Self::MessageQueueStart => "messageQueue/start",
            Self::EventSubscribe => "event/subscribe",
            Self::EventUnsubscribe => "event/unsubscribe",
            Self::ModelList => "model/list",
            Self::ConfigGet => "config/get",
            Self::ConfigUpdate => "config/update",
            Self::ConfigDelete => "config/delete",
            Self::ConfigReload => "config/reload",
            Self::FileUpload => "file/upload",
            Self::FileList => "file/list",
            Self::FileRead => "file/read",
            Self::FileUpdate => "file/update",
            Self::FileDelete => "file/delete",
            Self::MemoryFileList => "memory/file/list",
            Self::MemoryFileRead => "memory/file/read",
            Self::MemoryFileWrite => "memory/file/write",
            Self::WorkspaceDocList => "workspace/doc/list",
            Self::WorkspaceDocRead => "workspace/doc/read",
            Self::WorkspaceDocWrite => "workspace/doc/write",
            Self::SkillsList => "skills/list",
            Self::SkillsRefresh => "skills/refresh",
            Self::SkillsSuggest => "skills/suggest",
            Self::SkillsRead => "skills/read",
            Self::SkillsResolve => "skills/resolve",
            Self::PluginSourceList => "plugin/source/list",
            Self::PluginSourceAdd => "plugin/source/add",
            Self::PluginSourceRefresh => "plugin/source/refresh",
            Self::PluginSourceRemove => "plugin/source/remove",
            Self::PluginSearch => "plugin/search",
            Self::PluginList => "plugin/list",
            Self::PluginRead => "plugin/read",
            Self::PluginInstall => "plugin/install",
            Self::PluginUpdate => "plugin/update",
            Self::PluginUninstall => "plugin/uninstall",
            Self::PluginEnable => "plugin/enable",
            Self::PluginDisable => "plugin/disable",
            Self::AuthLogout => "auth/logout",
            Self::AuthLoginStart => "auth/login/start",
            Self::AuthLoginStatus => "auth/login/status",
            Self::AuthLoginCancel => "auth/login/cancel",
            Self::AuthSessionRead => "auth/session/read",
            Self::AuthSessionClear => "auth/session/clear",
            Self::UserCurrent => "user/current",
            Self::MemorySearch => "memory/search",
            Self::MemoryGet => "memory/get",
        }
    }
}

impl fmt::Display for ClientMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ClientMethod {
    type Err = AppServerErrorBody;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "initialize" => Ok(Self::Initialize),
            "profile/read" => Ok(Self::ProfileRead),
            "session/list" => Ok(Self::SessionList),
            "session/get" => Ok(Self::SessionGet),
            "session/patch" => Ok(Self::SessionPatch),
            "session/delete" => Ok(Self::SessionDelete),
            "session/close" => Ok(Self::SessionClose),
            "session/compact" => Ok(Self::SessionCompact),
            "session/clear" => Ok(Self::SessionClear),
            "session/active_turn" => Ok(Self::SessionActiveTurn),
            "session/history" => Ok(Self::SessionHistory),
            "timeline/recover" => Ok(Self::TimelineRecover),
            "project/create" => Ok(Self::ProjectCreate),
            "project/list" => Ok(Self::ProjectList),
            "project/read" => Ok(Self::ProjectRead),
            "project/patch" => Ok(Self::ProjectPatch),
            "project/delete" => Ok(Self::ProjectDelete),
            "fs/directory/read" => Ok(Self::FsDirectoryRead),
            "fs/directory/create" => Ok(Self::FsDirectoryCreate),
            "turn/start" => Ok(Self::TurnStart),
            "turn/interrupt" => Ok(Self::TurnInterrupt),
            "turn/input" => Ok(Self::TurnInput),
            "turn/retry" => Ok(Self::TurnRetry),
            "messageQueue/add" => Ok(Self::MessageQueueAdd),
            "messageQueue/list" => Ok(Self::MessageQueueList),
            "messageQueue/update" => Ok(Self::MessageQueueUpdate),
            "messageQueue/delete" => Ok(Self::MessageQueueDelete),
            "messageQueue/reorder" => Ok(Self::MessageQueueReorder),
            "messageQueue/start" => Ok(Self::MessageQueueStart),
            "event/subscribe" => Ok(Self::EventSubscribe),
            "event/unsubscribe" => Ok(Self::EventUnsubscribe),
            "model/list" => Ok(Self::ModelList),
            "config/get" => Ok(Self::ConfigGet),
            "config/update" => Ok(Self::ConfigUpdate),
            "config/delete" => Ok(Self::ConfigDelete),
            "config/reload" => Ok(Self::ConfigReload),
            "file/upload" => Ok(Self::FileUpload),
            "file/list" => Ok(Self::FileList),
            "file/read" => Ok(Self::FileRead),
            "file/update" => Ok(Self::FileUpdate),
            "file/delete" => Ok(Self::FileDelete),
            "memory/file/list" => Ok(Self::MemoryFileList),
            "memory/file/read" => Ok(Self::MemoryFileRead),
            "memory/file/write" => Ok(Self::MemoryFileWrite),
            "workspace/doc/list" => Ok(Self::WorkspaceDocList),
            "workspace/doc/read" => Ok(Self::WorkspaceDocRead),
            "workspace/doc/write" => Ok(Self::WorkspaceDocWrite),
            "skills/list" => Ok(Self::SkillsList),
            "skills/refresh" => Ok(Self::SkillsRefresh),
            "skills/suggest" => Ok(Self::SkillsSuggest),
            "skills/read" => Ok(Self::SkillsRead),
            "skills/resolve" => Ok(Self::SkillsResolve),
            "plugin/source/list" => Ok(Self::PluginSourceList),
            "plugin/source/add" => Ok(Self::PluginSourceAdd),
            "plugin/source/refresh" => Ok(Self::PluginSourceRefresh),
            "plugin/source/remove" => Ok(Self::PluginSourceRemove),
            "plugin/search" => Ok(Self::PluginSearch),
            "plugin/list" => Ok(Self::PluginList),
            "plugin/read" => Ok(Self::PluginRead),
            "plugin/install" => Ok(Self::PluginInstall),
            "plugin/update" => Ok(Self::PluginUpdate),
            "plugin/uninstall" => Ok(Self::PluginUninstall),
            "plugin/enable" => Ok(Self::PluginEnable),
            "plugin/disable" => Ok(Self::PluginDisable),
            "auth/logout" => Ok(Self::AuthLogout),
            "auth/login/start" => Ok(Self::AuthLoginStart),
            "auth/login/status" => Ok(Self::AuthLoginStatus),
            "auth/login/cancel" => Ok(Self::AuthLoginCancel),
            "auth/session/read" => Ok(Self::AuthSessionRead),
            "auth/session/clear" => Ok(Self::AuthSessionClear),
            "user/current" => Ok(Self::UserCurrent),
            "memory/search" => Ok(Self::MemorySearch),
            "memory/get" => Ok(Self::MemoryGet),
            _ => Err(AppServerErrorBody::method_not_found(value)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ClientRequestParams {
    #[serde(rename = "initialize")]
    Initialize(InitializeParams),
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
        let params_value = params.unwrap_or(serde_json::Value::Object(Default::default()));
        let params = match method_kind {
            ClientMethod::Initialize => ClientRequestParams::Initialize(decode(params_value)?),
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
