use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use ts_rs::TS;

use crate::errors::AppServerErrorBody;
use crate::version::ProtocolVersion;
use talos_error::{TalosError, TalosResult};
use talos_skills::{
    SkillDocument, SkillLoadError, SkillScope, SkillSelector, SkillSuggestion, SkillSummary,
    SkillWarning,
};
use talos_types::talos::{
    SessionHistorySnapshot, TalosMessageRequest, TalosMessageRetryRequest, TimelineEventEnvelope,
};

pub const DEFERRED_CLIENT_METHODS: &[DeferredClientMethod] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredClientMethod {
    pub method: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub enum ClientMethod {
    #[serde(rename = "initialize")]
    Initialize,
    #[serde(rename = "session.list")]
    SessionList,
    #[serde(rename = "session.read")]
    SessionRead,
    #[serde(rename = "session.patch")]
    SessionPatch,
    #[serde(rename = "session.delete")]
    SessionDelete,
    #[serde(rename = "session.close")]
    SessionClose,
    #[serde(rename = "session.compact")]
    SessionCompact,
    #[serde(rename = "session.clear")]
    SessionClear,
    #[serde(rename = "session.active_turn")]
    SessionActiveTurn,
    #[serde(rename = "session.history")]
    SessionHistory,
    #[serde(rename = "session.recover")]
    SessionRecover,
    #[serde(rename = "project.create")]
    ProjectCreate,
    #[serde(rename = "project.list")]
    ProjectList,
    #[serde(rename = "project.read")]
    ProjectRead,
    #[serde(rename = "project.patch")]
    ProjectPatch,
    #[serde(rename = "project.delete")]
    ProjectDelete,
    #[serde(rename = "fs.directory.read")]
    FsDirectoryRead,
    #[serde(rename = "fs.directory.create")]
    FsDirectoryCreate,
    #[serde(rename = "turn.start")]
    TurnStart,
    #[serde(rename = "turn.interrupt")]
    TurnInterrupt,
    #[serde(rename = "turn.retry")]
    TurnRetry,
    #[serde(rename = "event.subscribe")]
    EventSubscribe,
    #[serde(rename = "event.unsubscribe")]
    EventUnsubscribe,
    #[serde(rename = "tool.dynamic_result.submit")]
    ToolDynamicResultSubmit,
    #[serde(rename = "model.list")]
    ModelList,
    #[serde(rename = "config.read")]
    ConfigRead,
    #[serde(rename = "config.value.write")]
    ConfigValueWrite,
    #[serde(rename = "config.batch.write")]
    ConfigBatchWrite,
    #[serde(rename = "config.reload")]
    ConfigReload,
    #[serde(rename = "memory.file.list")]
    MemoryFileList,
    #[serde(rename = "memory.file.read")]
    MemoryFileRead,
    #[serde(rename = "memory.file.write")]
    MemoryFileWrite,
    #[serde(rename = "workspace.doc.list")]
    WorkspaceDocList,
    #[serde(rename = "workspace.doc.read")]
    WorkspaceDocRead,
    #[serde(rename = "workspace.doc.write")]
    WorkspaceDocWrite,
    #[serde(rename = "workspace.info")]
    WorkspaceInfo,
    #[serde(rename = "skills.list")]
    SkillsList,
    #[serde(rename = "skills.refresh")]
    SkillsRefresh,
    #[serde(rename = "skills.suggest")]
    SkillsSuggest,
    #[serde(rename = "skills.read")]
    SkillsRead,
    #[serde(rename = "skills.resolve")]
    SkillsResolve,
    #[serde(rename = "skills.install")]
    SkillsInstall,
    #[serde(rename = "skills.uninstall")]
    SkillsUninstall,
    #[serde(rename = "skills.enable")]
    SkillsEnable,
    #[serde(rename = "skills.disable")]
    SkillsDisable,
    #[serde(rename = "app.list")]
    AppList,
    #[serde(rename = "tool.list")]
    ToolList,
    #[serde(rename = "auth.logout")]
    AuthLogout,
    #[serde(rename = "auth.login.start")]
    AuthLoginStart,
    #[serde(rename = "auth.login.status")]
    AuthLoginStatus,
    #[serde(rename = "auth.login.cancel")]
    AuthLoginCancel,
    #[serde(rename = "auth.session.read")]
    AuthSessionRead,
    #[serde(rename = "auth.session.store")]
    AuthSessionStore,
    #[serde(rename = "auth.session.clear")]
    AuthSessionClear,
    #[serde(rename = "user.current")]
    UserCurrent,
    #[serde(rename = "memory.search")]
    MemorySearch,
}

impl ClientMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::SessionList => "session.list",
            Self::SessionRead => "session.read",
            Self::SessionPatch => "session.patch",
            Self::SessionDelete => "session.delete",
            Self::SessionClose => "session.close",
            Self::SessionCompact => "session.compact",
            Self::SessionClear => "session.clear",
            Self::SessionActiveTurn => "session.active_turn",
            Self::SessionHistory => "session.history",
            Self::SessionRecover => "session.recover",
            Self::ProjectCreate => "project.create",
            Self::ProjectList => "project.list",
            Self::ProjectRead => "project.read",
            Self::ProjectPatch => "project.patch",
            Self::ProjectDelete => "project.delete",
            Self::FsDirectoryRead => "fs.directory.read",
            Self::FsDirectoryCreate => "fs.directory.create",
            Self::TurnStart => "turn.start",
            Self::TurnInterrupt => "turn.interrupt",
            Self::TurnRetry => "turn.retry",
            Self::EventSubscribe => "event.subscribe",
            Self::EventUnsubscribe => "event.unsubscribe",
            Self::ToolDynamicResultSubmit => "tool.dynamic_result.submit",
            Self::ModelList => "model.list",
            Self::ConfigRead => "config.read",
            Self::ConfigValueWrite => "config.value.write",
            Self::ConfigBatchWrite => "config.batch.write",
            Self::ConfigReload => "config.reload",
            Self::MemoryFileList => "memory.file.list",
            Self::MemoryFileRead => "memory.file.read",
            Self::MemoryFileWrite => "memory.file.write",
            Self::WorkspaceDocList => "workspace.doc.list",
            Self::WorkspaceDocRead => "workspace.doc.read",
            Self::WorkspaceDocWrite => "workspace.doc.write",
            Self::WorkspaceInfo => "workspace.info",
            Self::SkillsList => "skills.list",
            Self::SkillsRefresh => "skills.refresh",
            Self::SkillsSuggest => "skills.suggest",
            Self::SkillsRead => "skills.read",
            Self::SkillsResolve => "skills.resolve",
            Self::SkillsInstall => "skills.install",
            Self::SkillsUninstall => "skills.uninstall",
            Self::SkillsEnable => "skills.enable",
            Self::SkillsDisable => "skills.disable",
            Self::AppList => "app.list",
            Self::ToolList => "tool.list",
            Self::AuthLogout => "auth.logout",
            Self::AuthLoginStart => "auth.login.start",
            Self::AuthLoginStatus => "auth.login.status",
            Self::AuthLoginCancel => "auth.login.cancel",
            Self::AuthSessionRead => "auth.session.read",
            Self::AuthSessionStore => "auth.session.store",
            Self::AuthSessionClear => "auth.session.clear",
            Self::UserCurrent => "user.current",
            Self::MemorySearch => "memory.search",
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
            "session.list" => Ok(Self::SessionList),
            "session.read" => Ok(Self::SessionRead),
            "session.patch" => Ok(Self::SessionPatch),
            "session.delete" => Ok(Self::SessionDelete),
            "session.close" => Ok(Self::SessionClose),
            "session.compact" => Ok(Self::SessionCompact),
            "session.clear" => Ok(Self::SessionClear),
            "session.active_turn" => Ok(Self::SessionActiveTurn),
            "session.history" => Ok(Self::SessionHistory),
            "session.recover" => Ok(Self::SessionRecover),
            "project.create" => Ok(Self::ProjectCreate),
            "project.list" => Ok(Self::ProjectList),
            "project.read" => Ok(Self::ProjectRead),
            "project.patch" => Ok(Self::ProjectPatch),
            "project.delete" => Ok(Self::ProjectDelete),
            "fs.directory.read" => Ok(Self::FsDirectoryRead),
            "fs.directory.create" => Ok(Self::FsDirectoryCreate),
            "turn.start" => Ok(Self::TurnStart),
            "turn.interrupt" => Ok(Self::TurnInterrupt),
            "turn.retry" => Ok(Self::TurnRetry),
            "event.subscribe" => Ok(Self::EventSubscribe),
            "event.unsubscribe" => Ok(Self::EventUnsubscribe),
            "tool.dynamic_result.submit" => Ok(Self::ToolDynamicResultSubmit),
            "model.list" => Ok(Self::ModelList),
            "config.read" => Ok(Self::ConfigRead),
            "config.value.write" => Ok(Self::ConfigValueWrite),
            "config.batch.write" => Ok(Self::ConfigBatchWrite),
            "config.reload" => Ok(Self::ConfigReload),
            "memory.file.list" => Ok(Self::MemoryFileList),
            "memory.file.read" => Ok(Self::MemoryFileRead),
            "memory.file.write" => Ok(Self::MemoryFileWrite),
            "workspace.doc.list" => Ok(Self::WorkspaceDocList),
            "workspace.doc.read" => Ok(Self::WorkspaceDocRead),
            "workspace.doc.write" => Ok(Self::WorkspaceDocWrite),
            "workspace.info" => Ok(Self::WorkspaceInfo),
            "skills.list" => Ok(Self::SkillsList),
            "skills.refresh" => Ok(Self::SkillsRefresh),
            "skills.suggest" => Ok(Self::SkillsSuggest),
            "skills.read" => Ok(Self::SkillsRead),
            "skills.resolve" => Ok(Self::SkillsResolve),
            "skills.install" => Ok(Self::SkillsInstall),
            "skills.uninstall" => Ok(Self::SkillsUninstall),
            "skills.enable" => Ok(Self::SkillsEnable),
            "skills.disable" => Ok(Self::SkillsDisable),
            "app.list" => Ok(Self::AppList),
            "tool.list" => Ok(Self::ToolList),
            "auth.logout" => Ok(Self::AuthLogout),
            "auth.login.start" => Ok(Self::AuthLoginStart),
            "auth.login.status" => Ok(Self::AuthLoginStatus),
            "auth.login.cancel" => Ok(Self::AuthLoginCancel),
            "auth.session.read" => Ok(Self::AuthSessionRead),
            "auth.session.store" => Ok(Self::AuthSessionStore),
            "auth.session.clear" => Ok(Self::AuthSessionClear),
            "user.current" => Ok(Self::UserCurrent),
            "memory.search" => Ok(Self::MemorySearch),
            _ => Err(AppServerErrorBody::method_not_found(value)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ClientRequestParams {
    #[serde(rename = "initialize")]
    Initialize(InitializeParams),
    #[serde(rename = "session.list")]
    SessionList(SessionListParams),
    #[serde(rename = "session.read")]
    SessionRead(SessionReadParams),
    #[serde(rename = "session.patch")]
    SessionPatch(SessionPatchParams),
    #[serde(rename = "session.delete")]
    SessionDelete(SessionDeleteParams),
    #[serde(rename = "session.close")]
    SessionClose(SessionCloseParams),
    #[serde(rename = "session.compact")]
    SessionCompact(SessionCompactParams),
    #[serde(rename = "session.clear")]
    SessionClear(SessionClearParams),
    #[serde(rename = "session.active_turn")]
    SessionActiveTurn(SessionActiveTurnParams),
    #[serde(rename = "session.history")]
    SessionHistory(SessionHistoryParams),
    #[serde(rename = "session.recover")]
    SessionRecover(TimelineRecoveryParams),
    #[serde(rename = "project.create")]
    ProjectCreate(ProjectCreateParams),
    #[serde(rename = "project.list")]
    ProjectList(ProjectListParams),
    #[serde(rename = "project.read")]
    ProjectRead(ProjectReadParams),
    #[serde(rename = "project.patch")]
    ProjectPatch(ProjectPatchParams),
    #[serde(rename = "project.delete")]
    ProjectDelete(ProjectDeleteParams),
    #[serde(rename = "fs.directory.read")]
    FsDirectoryRead(FsDirectoryReadParams),
    #[serde(rename = "fs.directory.create")]
    FsDirectoryCreate(FsDirectoryCreateParams),
    #[serde(rename = "turn.start")]
    TurnStart(TurnStartParams),
    #[serde(rename = "turn.interrupt")]
    TurnInterrupt(TurnInterruptParams),
    #[serde(rename = "turn.retry")]
    TurnRetry(TurnRetryParams),
    #[serde(rename = "event.subscribe")]
    EventSubscribe(EventSubscribeParams),
    #[serde(rename = "event.unsubscribe")]
    EventUnsubscribe(EventUnsubscribeParams),
    #[serde(rename = "tool.dynamic_result.submit")]
    ToolDynamicResultSubmit(ToolDynamicResultSubmitParams),
    #[serde(rename = "model.list")]
    ModelList(ModelListParams),
    #[serde(rename = "config.read")]
    ConfigRead(ConfigReadParams),
    #[serde(rename = "config.value.write")]
    ConfigValueWrite(ConfigValueWriteParams),
    #[serde(rename = "config.batch.write")]
    ConfigBatchWrite(ConfigBatchWriteParams),
    #[serde(rename = "config.reload")]
    ConfigReload(ConfigReloadParams),
    #[serde(rename = "memory.file.list")]
    MemoryFileList(MemoryFileListParams),
    #[serde(rename = "memory.file.read")]
    MemoryFileRead(MemoryFileReadParams),
    #[serde(rename = "memory.file.write")]
    MemoryFileWrite(MemoryFileWriteParams),
    #[serde(rename = "workspace.doc.list")]
    WorkspaceDocList(WorkspaceDocListParams),
    #[serde(rename = "workspace.doc.read")]
    WorkspaceDocRead(WorkspaceDocReadParams),
    #[serde(rename = "workspace.doc.write")]
    WorkspaceDocWrite(WorkspaceDocWriteParams),
    #[serde(rename = "workspace.info")]
    WorkspaceInfo(WorkspaceInfoParams),
    #[serde(rename = "skills.list")]
    SkillsList(SkillsListParams),
    #[serde(rename = "skills.refresh")]
    SkillsRefresh(SkillsRefreshParams),
    #[serde(rename = "skills.suggest")]
    SkillsSuggest(SkillsSuggestParams),
    #[serde(rename = "skills.read")]
    SkillsRead(SkillsReadParams),
    #[serde(rename = "skills.resolve")]
    SkillsResolve(SkillsResolveParams),
    #[serde(rename = "skills.install")]
    SkillsInstall(SkillsInstallParams),
    #[serde(rename = "skills.uninstall")]
    SkillsUninstall(SkillsUninstallParams),
    #[serde(rename = "skills.enable")]
    SkillsEnable(SkillsEnableParams),
    #[serde(rename = "skills.disable")]
    SkillsDisable(SkillsDisableParams),
    #[serde(rename = "app.list")]
    AppList(AppListParams),
    #[serde(rename = "tool.list")]
    ToolList(ToolListParams),
    #[serde(rename = "auth.logout")]
    AuthLogout(AuthLogoutParams),
    #[serde(rename = "auth.login.start")]
    AuthLoginStart(talos_auth::CloudLoginStartRequest),
    #[serde(rename = "auth.login.status")]
    AuthLoginStatus(talos_auth::CloudLoginStatusRequest),
    #[serde(rename = "auth.login.cancel")]
    AuthLoginCancel(talos_auth::CloudLoginCancelRequest),
    #[serde(rename = "auth.session.read")]
    AuthSessionRead(AuthSessionReadParams),
    #[serde(rename = "auth.session.store")]
    AuthSessionStore(AuthSessionStoreParams),
    #[serde(rename = "auth.session.clear")]
    AuthSessionClear(AuthSessionClearParams),
    #[serde(rename = "user.current")]
    UserCurrent(UserCurrentParams),
    #[serde(rename = "memory.search")]
    MemorySearch(MemorySearchParams),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
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
            ClientMethod::SessionList => ClientRequestParams::SessionList(decode(params_value)?),
            ClientMethod::SessionRead => ClientRequestParams::SessionRead(decode(params_value)?),
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
            ClientMethod::SessionRecover => {
                ClientRequestParams::SessionRecover(decode(params_value)?)
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
            ClientMethod::TurnRetry => ClientRequestParams::TurnRetry(decode(params_value)?),
            ClientMethod::EventSubscribe => {
                ClientRequestParams::EventSubscribe(decode(params_value)?)
            }
            ClientMethod::EventUnsubscribe => {
                ClientRequestParams::EventUnsubscribe(decode(params_value)?)
            }
            ClientMethod::ToolDynamicResultSubmit => {
                ClientRequestParams::ToolDynamicResultSubmit(decode(params_value)?)
            }
            ClientMethod::ModelList => ClientRequestParams::ModelList(decode(params_value)?),
            ClientMethod::ConfigRead => ClientRequestParams::ConfigRead(decode(params_value)?),
            ClientMethod::ConfigValueWrite => {
                ClientRequestParams::ConfigValueWrite(decode(params_value)?)
            }
            ClientMethod::ConfigBatchWrite => {
                ClientRequestParams::ConfigBatchWrite(decode(params_value)?)
            }
            ClientMethod::ConfigReload => ClientRequestParams::ConfigReload(decode(params_value)?),
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
            ClientMethod::WorkspaceInfo => {
                ClientRequestParams::WorkspaceInfo(decode(params_value)?)
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
            ClientMethod::SkillsInstall => {
                ClientRequestParams::SkillsInstall(decode(params_value)?)
            }
            ClientMethod::SkillsUninstall => {
                ClientRequestParams::SkillsUninstall(decode(params_value)?)
            }
            ClientMethod::SkillsEnable => ClientRequestParams::SkillsEnable(decode(params_value)?),
            ClientMethod::SkillsDisable => {
                ClientRequestParams::SkillsDisable(decode(params_value)?)
            }
            ClientMethod::AppList => ClientRequestParams::AppList(decode(params_value)?),
            ClientMethod::ToolList => ClientRequestParams::ToolList(decode(params_value)?),
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
            ClientMethod::AuthSessionStore => {
                ClientRequestParams::AuthSessionStore(decode(params_value)?)
            }
            ClientMethod::AuthSessionClear => {
                ClientRequestParams::AuthSessionClear(decode(params_value)?)
            }
            ClientMethod::UserCurrent => ClientRequestParams::UserCurrent(decode(params_value)?),
            ClientMethod::MemorySearch => ClientRequestParams::MemorySearch(decode(params_value)?),
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct SessionListParams {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionReadParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct SessionPatchParams {
    pub session_id: String,
    #[serde(default)]
    pub project_id: Option<OptionalStringPatch>,
    #[serde(default)]
    pub current_directory: Option<OptionalPathPatch>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionDeleteParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionCloseParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionCompactParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionClearParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionActiveTurnParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionHistoryParams {
    pub session_id: String,
}

pub type SessionHistoryResponse = SessionHistorySnapshot;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TimelineRecoveryParams {
    pub session_id: String,
    #[serde(default)]
    pub after_event_id: Option<String>,
    #[serde(default)]
    pub replay_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineRecoverySnapshot {
    pub replay: TimelineReplay,
    pub active_turn: TimelineActiveTurnSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineReplay {
    pub events: Vec<TimelineEventEnvelope>,
    #[serde(default)]
    pub high_water_mark_event_id: Option<String>,
    pub last_replay_seq: i64,
    pub cursor_status: TimelineCursorStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineCursorStatus {
    Current,
    FromBeginning,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineActiveTurnSnapshot {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    pub status: TimelineActiveTurnStatus,
    #[serde(default)]
    pub last_event_id: Option<String>,
    #[serde(default)]
    pub pending_user_action: Option<TimelinePendingUserAction>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineActiveTurnStatus {
    Running,
    WaitingUserAction,
    Cancelling,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelinePendingUserAction {
    DynamicToolResult(TimelinePendingDynamicToolResult),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TimelinePendingDynamicToolResult {
    #[serde(default)]
    pub item_id: Option<String>,
    pub call_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProjectCreateParams {
    pub name: String,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ProjectListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProjectReadParams {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ProjectPatchParams {
    pub project_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProjectDeleteParams {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct FsDirectoryReadParams {
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct FsDirectoryCreateParams {
    pub parent: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TurnStartParams {
    pub session_id: String,
    pub request: TalosMessageRequest,
    #[serde(default)]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageCommandStatus {
    Pending,
    Accepted,
    Failed,
    FailedNoFirstEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TurnStartResponse {
    pub request_id: String,
    #[serde(default)]
    pub message_id: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub status: MessageCommandStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TurnInterruptParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TurnRetryParams {
    pub session_id: String,
    pub request: TalosMessageRetryRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct EventSubscribeParams {
    pub session_id: String,
    #[serde(default)]
    pub since_event_id: Option<String>,
    #[serde(default)]
    pub replay_limit: Option<usize>,
    #[serde(default)]
    pub close_when_idle: Option<bool>,
    #[serde(default)]
    pub close_on_terminal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct EventSubscribeResponse {
    pub subscription_id: String,
    pub session_id: String,
    #[serde(default)]
    pub high_water_mark_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct EventUnsubscribeParams {
    pub subscription_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct EventUnsubscribeResponse {
    pub subscription_id: String,
    pub unsubscribed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ToolDynamicResultSubmitParams {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ModelListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ConfigReadParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigValueWriteParams {
    pub path: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigBatchWriteParams {
    pub entries: Vec<ConfigValueWriteParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ConfigReloadParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct MemoryFileListParams {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileReadParams {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileWriteParams {
    pub path: PathBuf,
    pub content: String,
    #[serde(default)]
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct WorkspaceDocListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocReadParams {
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocWriteParams {
    pub kind: String,
    pub content: String,
    #[serde(default)]
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct WorkspaceInfoParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceInfoResponse {
    pub user_id: String,
    #[serde(default)]
    pub username: Option<String>,
    pub base_workspace_dir: String,
    pub user_workspace_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct SkillsListParams {
    #[serde(default)]
    pub include_disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct SkillsRefreshParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct SkillsSuggestParams {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ProtocolSkillSelector {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SkillsReadParams {
    pub selector: ProtocolSkillSelector,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct SkillsResolveParams {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub selected: Vec<ProtocolSkillSelector>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SkillsInstallParams {
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SkillsUninstallParams {
    pub selector: ProtocolSkillSelector,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SkillsEnableParams {
    pub selector: ProtocolSkillSelector,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SkillsDisableParams {
    pub selector: ProtocolSkillSelector,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolSkillScope {
    User,
    External,
    Project,
    Plugin,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProtocolSkillMetadata {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub argument_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ProtocolSkillSummary {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub short_description: Option<String>,
    pub root: PathBuf,
    pub path: PathBuf,
    pub scope: ProtocolSkillScope,
    #[serde(default)]
    pub source_plugin: Option<String>,
    pub enabled: bool,
    pub read_only: bool,
    #[serde(default)]
    pub metadata: Option<ProtocolSkillMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ProtocolSkillSuggestion {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub short_description: Option<String>,
    pub root: PathBuf,
    pub path: PathBuf,
    pub scope: ProtocolSkillScope,
    #[serde(default)]
    pub source_plugin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ProtocolSkillLoadError {
    pub root: PathBuf,
    #[serde(default)]
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct SkillsListResponse {
    pub data: Vec<ProtocolSkillSummary>,
    #[serde(default)]
    pub errors: Vec<ProtocolSkillLoadError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct SkillsSuggestResponse {
    pub data: Vec<ProtocolSkillSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ProtocolSkillDocument {
    pub summary: ProtocolSkillSummary,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProtocolSkillWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct SkillsResolveResponse {
    pub data: Vec<ProtocolSkillSummary>,
    #[serde(default)]
    pub warnings: Vec<ProtocolSkillWarning>,
}

pub fn protocol_skill_selector(selector: ProtocolSkillSelector) -> TalosResult<SkillSelector> {
    match (selector.name, selector.path) {
        (Some(name), Some(path)) if !name.trim().is_empty() => {
            Ok(SkillSelector::selection(name, path))
        }
        (Some(name), None) if !name.trim().is_empty() => Ok(SkillSelector::by_name(name)),
        (None, Some(path)) => Ok(SkillSelector::by_path(path)),
        _ => Err(TalosError::ValidationError(
            "skill selector requires name, path, or both".to_string(),
        )),
    }
}

pub fn protocol_skill_document(document: SkillDocument) -> ProtocolSkillDocument {
    ProtocolSkillDocument {
        summary: protocol_skill_summary(document.summary),
        contents: document.contents,
    }
}

pub fn protocol_skill_summary(skill: SkillSummary) -> ProtocolSkillSummary {
    ProtocolSkillSummary {
        name: skill.name,
        description: skill.description,
        short_description: skill.short_description,
        root: skill.root,
        path: skill.path,
        scope: protocol_skill_scope(skill.scope),
        source_plugin: skill.source_plugin,
        enabled: skill.enabled,
        read_only: skill.read_only,
        metadata: skill.metadata.map(|metadata| ProtocolSkillMetadata {
            display_name: metadata.display_name,
            icon: metadata.icon,
            author: metadata.author,
            version: metadata.version,
            argument_hint: metadata.argument_hint,
        }),
    }
}

pub fn protocol_skill_suggestion(skill: SkillSuggestion) -> ProtocolSkillSuggestion {
    ProtocolSkillSuggestion {
        name: skill.name,
        description: skill.description,
        short_description: skill.short_description,
        root: skill.root,
        path: skill.path,
        scope: protocol_skill_scope(skill.scope),
        source_plugin: skill.source_plugin,
    }
}

pub fn protocol_skill_load_error(error: SkillLoadError) -> ProtocolSkillLoadError {
    ProtocolSkillLoadError {
        root: error.root,
        path: error.path,
        message: error.message,
    }
}

pub fn protocol_skill_warning(warning: SkillWarning) -> ProtocolSkillWarning {
    ProtocolSkillWarning {
        code: warning.code,
        message: warning.message,
    }
}

pub fn protocol_skill_scope(scope: SkillScope) -> ProtocolSkillScope {
    match scope {
        SkillScope::User => ProtocolSkillScope::User,
        SkillScope::External => ProtocolSkillScope::External,
        SkillScope::Project => ProtocolSkillScope::Project,
        SkillScope::Plugin => ProtocolSkillScope::Plugin,
        SkillScope::System => ProtocolSkillScope::System,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AppListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ToolListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AuthLogoutParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AuthSessionReadParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionReadResponse {
    #[serde(default)]
    pub session: Option<AuthSessionSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionStoreParams {
    pub session: AuthSessionSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AuthSessionClearParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionSnapshot {
    pub schema_version: u32,
    pub cloud_api_base_url: String,
    pub access_token: String,
    pub token_type: String,
    #[serde(default)]
    pub expires_at_ms: Option<String>,
    pub last_used_at_ms: String,
    pub user: AuthSessionUserSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionUserSnapshot {
    pub user_id: String,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub role: talos_auth::UserRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct UserCurrentParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct MemorySearchParams {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionalStringPatch {
    Set { value: String },
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionalPathPatch {
    Set { path: PathBuf },
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProtocolTalosContext {
    pub source_type: String,
    pub provider: String,
    pub channel_id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub message_id: String,
    pub trace_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use talos_types::talos::TalosInputItem;

    #[test]
    fn method_parses_dot_name() {
        assert_eq!(
            ClientMethod::from_str("turn.start").unwrap(),
            ClientMethod::TurnStart
        );
    }

    #[test]
    fn decodes_typed_request_params() {
        let request = ClientRequest::decode(
            "session.read",
            Some(json!({
                "session_id": "s1"
            })),
        )
        .unwrap();
        assert!(matches!(
            request.params,
            ClientRequestParams::SessionRead(SessionReadParams { .. })
        ));
    }

    #[test]
    fn turn_start_input_uses_shared_talos_input_item() {
        let value = json!({
            "type": "skill_selection",
            "name": "review",
            "path": "/tmp/review/SKILL.md"
        });

        let item: TalosInputItem =
            serde_json::from_value(value).expect("skill selection should decode");

        assert_eq!(
            item,
            TalosInputItem::SkillSelection {
                name: "review".to_string(),
                path: "/tmp/review/SKILL.md".to_string(),
            }
        );
    }

    #[test]
    fn skills_methods_parse_without_legacy_skill_list_alias() {
        assert_eq!(
            ClientMethod::from_str("skills.list").unwrap(),
            ClientMethod::SkillsList
        );
        assert_eq!(
            ClientMethod::from_str("skills.refresh").unwrap(),
            ClientMethod::SkillsRefresh
        );
        assert_eq!(
            ClientMethod::from_str("skills.suggest").unwrap(),
            ClientMethod::SkillsSuggest
        );
        assert!(ClientMethod::from_str("skill.list").is_err());

        let request = ClientRequest::decode(
            "skills.resolve",
            Some(json!({
                "text": "Use $review",
                "selected": [{ "name": "review", "path": "/tmp/review/SKILL.md" }]
            })),
        )
        .expect("skills resolve request should decode");
        assert!(matches!(
            request.params,
            ClientRequestParams::SkillsResolve(SkillsResolveParams { .. })
        ));

        let request = ClientRequest::decode("skills.refresh", None)
            .expect("skills refresh request should decode");
        assert!(matches!(
            request.params,
            ClientRequestParams::SkillsRefresh(SkillsRefreshParams {})
        ));
    }

    #[test]
    fn deferred_methods_are_empty_after_p2_freeze() {
        assert!(DEFERRED_CLIENT_METHODS.is_empty());
        assert_eq!(
            ClientMethod::from_str("event.unsubscribe").unwrap(),
            ClientMethod::EventUnsubscribe
        );
        assert_eq!(
            ClientMethod::from_str("tool.dynamic_result.submit").unwrap(),
            ClientMethod::ToolDynamicResultSubmit
        );
        assert_eq!(
            ClientMethod::from_str("session.close").unwrap(),
            ClientMethod::SessionClose
        );
        assert!(ClientMethod::from_str("account.get").is_err());
        assert!(ClientMethod::from_str("admin.session.list").is_err());
        assert!(ClientMethod::from_str("admin.project.list").is_err());
    }
}
