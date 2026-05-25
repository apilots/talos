use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use ts_rs::TS;

use crate::errors::AppServerErrorBody;
use crate::version::ProtocolVersion;
use talos_types::talos::{SessionHistorySnapshot, TimelineEventEnvelope};

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
    #[serde(rename = "session.create")]
    SessionCreate,
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
    #[serde(rename = "admin.session.list")]
    AdminSessionList,
    #[serde(rename = "admin.project.list")]
    AdminProjectList,
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
    #[serde(rename = "skill.list")]
    SkillList,
    #[serde(rename = "app.list")]
    AppList,
    #[serde(rename = "tool.list")]
    ToolList,
    #[serde(rename = "account.get")]
    AccountGet,
    #[serde(rename = "auth.logout")]
    AuthLogout,
    #[serde(rename = "user.current")]
    UserCurrent,
    #[serde(rename = "memory.search")]
    MemorySearch,
}

impl ClientMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::SessionCreate => "session.create",
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
            Self::TurnStart => "turn.start",
            Self::TurnInterrupt => "turn.interrupt",
            Self::TurnRetry => "turn.retry",
            Self::EventSubscribe => "event.subscribe",
            Self::EventUnsubscribe => "event.unsubscribe",
            Self::ToolDynamicResultSubmit => "tool.dynamic_result.submit",
            Self::AdminSessionList => "admin.session.list",
            Self::AdminProjectList => "admin.project.list",
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
            Self::SkillList => "skill.list",
            Self::AppList => "app.list",
            Self::ToolList => "tool.list",
            Self::AccountGet => "account.get",
            Self::AuthLogout => "auth.logout",
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
            "session.create" => Ok(Self::SessionCreate),
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
            "turn.start" => Ok(Self::TurnStart),
            "turn.interrupt" => Ok(Self::TurnInterrupt),
            "turn.retry" => Ok(Self::TurnRetry),
            "event.subscribe" => Ok(Self::EventSubscribe),
            "event.unsubscribe" => Ok(Self::EventUnsubscribe),
            "tool.dynamic_result.submit" => Ok(Self::ToolDynamicResultSubmit),
            "admin.session.list" => Ok(Self::AdminSessionList),
            "admin.project.list" => Ok(Self::AdminProjectList),
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
            "skill.list" => Ok(Self::SkillList),
            "app.list" => Ok(Self::AppList),
            "tool.list" => Ok(Self::ToolList),
            "account.get" => Ok(Self::AccountGet),
            "auth.logout" => Ok(Self::AuthLogout),
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
    #[serde(rename = "session.create")]
    SessionCreate(SessionCreateParams),
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
    #[serde(rename = "admin.session.list")]
    AdminSessionList(AdminSessionListParams),
    #[serde(rename = "admin.project.list")]
    AdminProjectList(AdminProjectListParams),
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
    #[serde(rename = "skill.list")]
    SkillList(SkillListParams),
    #[serde(rename = "app.list")]
    AppList(AppListParams),
    #[serde(rename = "tool.list")]
    ToolList(ToolListParams),
    #[serde(rename = "account.get")]
    AccountGet(AccountGetParams),
    #[serde(rename = "auth.logout")]
    AuthLogout(AuthLogoutParams),
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
            ClientMethod::SessionCreate => {
                ClientRequestParams::SessionCreate(decode(params_value)?)
            }
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
            ClientMethod::AdminSessionList => {
                ClientRequestParams::AdminSessionList(decode(params_value)?)
            }
            ClientMethod::AdminProjectList => {
                ClientRequestParams::AdminProjectList(decode(params_value)?)
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
            ClientMethod::SkillList => ClientRequestParams::SkillList(decode(params_value)?),
            ClientMethod::AppList => ClientRequestParams::AppList(decode(params_value)?),
            ClientMethod::ToolList => ClientRequestParams::ToolList(decode(params_value)?),
            ClientMethod::AccountGet => ClientRequestParams::AccountGet(decode(params_value)?),
            ClientMethod::AuthLogout => ClientRequestParams::AuthLogout(decode(params_value)?),
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
    pub actor_credentials: ActorCredentials,
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
    LocalLaunchToken { token: String },
    ExistingSessionToken { token: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SessionCreateParams {
    pub context: ProtocolTalosContext,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub current_directory: Option<PathBuf>,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TurnStartParams {
    pub session_id: String,
    pub input: Vec<ProtocolInputItem>,
    #[serde(default)]
    pub workspace_id: Option<String>,
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
    pub input: Vec<ProtocolInputItem>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub client_message_id: String,
    pub retry_of_message_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AdminSessionListParams {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AdminSessionPagination {
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSessionListResponse {
    pub data: Vec<talos_types::TalosSession>,
    pub pagination: AdminSessionPagination,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AdminProjectListParams {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminProjectListResponse {
    pub data: Vec<talos_types::ProjectRecord>,
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
pub struct SkillListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AppListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct ToolListParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AccountGetParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AuthLogoutParams {}

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolInputItem {
    Text { text: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(
            ClientMethod::from_str("admin.session.list").unwrap(),
            ClientMethod::AdminSessionList
        );
        assert_eq!(
            ClientMethod::from_str("admin.project.list").unwrap(),
            ClientMethod::AdminProjectList
        );
    }
}
