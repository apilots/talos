use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use ts_rs::TS;

use crate::errors::AppServerErrorBody;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub enum ClientMethod {
    #[serde(rename = "initialize")]
    Initialize,
    #[serde(rename = "desktop/workerIdentity")]
    DesktopWorkerIdentity,
    #[serde(rename = "desktop/workerEnroll")]
    DesktopWorkerEnroll,
    #[serde(rename = "sync/status")]
    SyncStatus,
    #[serde(rename = "sync/metrics")]
    SyncMetrics,
    #[serde(rename = "sync/enable")]
    SyncEnable,
    #[serde(rename = "sync/disable")]
    SyncDisable,
    #[serde(rename = "computer/list")]
    ComputerList,
    #[serde(rename = "computer/read")]
    ComputerRead,
    #[serde(rename = "computer/create")]
    ComputerCreate,
    #[serde(rename = "computer/rename")]
    ComputerRename,
    #[serde(rename = "computer/delete")]
    ComputerDelete,
    #[serde(rename = "computer/setStatus")]
    ComputerSetStatus,
    #[serde(rename = "computer/rotateCredential")]
    ComputerRotateCredential,
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
    #[serde(rename = "file/content")]
    FileContent,
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
    #[serde(rename = "workspace/info")]
    WorkspaceInfo,
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
    #[serde(rename = "mcp/server/list")]
    McpServerList,
    #[serde(rename = "mcp/server/credentials/update")]
    McpCredentialsUpdate,
    #[serde(rename = "mcp/server/auth/start")]
    McpAuthenticationStart,
    #[serde(rename = "mcp/server/auth/read")]
    McpAuthenticationRead,
    #[serde(rename = "mcp/server/import")]
    McpServerImport,
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
            Self::DesktopWorkerIdentity => "desktop/workerIdentity",
            Self::DesktopWorkerEnroll => "desktop/workerEnroll",
            Self::SyncMetrics => "sync/metrics",
            Self::SyncStatus => "sync/status",
            Self::SyncEnable => "sync/enable",
            Self::SyncDisable => "sync/disable",
            Self::ComputerList => "computer/list",
            Self::ComputerRead => "computer/read",
            Self::ComputerCreate => "computer/create",
            Self::ComputerRename => "computer/rename",
            Self::ComputerDelete => "computer/delete",
            Self::ComputerSetStatus => "computer/setStatus",
            Self::ComputerRotateCredential => "computer/rotateCredential",
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
            Self::FileContent => "file/content",
            Self::FileUpdate => "file/update",
            Self::FileDelete => "file/delete",
            Self::MemoryFileList => "memory/file/list",
            Self::MemoryFileRead => "memory/file/read",
            Self::MemoryFileWrite => "memory/file/write",
            Self::WorkspaceInfo => "workspace/info",
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
            Self::McpServerList => "mcp/server/list",
            Self::McpCredentialsUpdate => "mcp/server/credentials/update",
            Self::McpAuthenticationStart => "mcp/server/auth/start",
            Self::McpAuthenticationRead => "mcp/server/auth/read",
            Self::McpServerImport => "mcp/server/import",
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
            "desktop/workerIdentity" => Ok(Self::DesktopWorkerIdentity),
            "desktop/workerEnroll" => Ok(Self::DesktopWorkerEnroll),
            "sync/metrics" => Ok(Self::SyncMetrics),
            "sync/status" => Ok(Self::SyncStatus),
            "sync/enable" => Ok(Self::SyncEnable),
            "sync/disable" => Ok(Self::SyncDisable),
            "computer/list" => Ok(Self::ComputerList),
            "computer/read" => Ok(Self::ComputerRead),
            "computer/create" => Ok(Self::ComputerCreate),
            "computer/rename" => Ok(Self::ComputerRename),
            "computer/delete" => Ok(Self::ComputerDelete),
            "computer/setStatus" => Ok(Self::ComputerSetStatus),
            "computer/rotateCredential" => Ok(Self::ComputerRotateCredential),
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
            "file/content" => Ok(Self::FileContent),
            "file/update" => Ok(Self::FileUpdate),
            "file/delete" => Ok(Self::FileDelete),
            "memory/file/list" => Ok(Self::MemoryFileList),
            "memory/file/read" => Ok(Self::MemoryFileRead),
            "memory/file/write" => Ok(Self::MemoryFileWrite),
            "workspace/info" => Ok(Self::WorkspaceInfo),
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
            "mcp/server/list" => Ok(Self::McpServerList),
            "mcp/server/credentials/update" => Ok(Self::McpCredentialsUpdate),
            "mcp/server/auth/start" => Ok(Self::McpAuthenticationStart),
            "mcp/server/auth/read" => Ok(Self::McpAuthenticationRead),
            "mcp/server/import" => Ok(Self::McpServerImport),
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
