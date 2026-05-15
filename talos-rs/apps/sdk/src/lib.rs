//! Talos Rust SDK facade.
//!
//! The SDK exposes the local JSON-RPC stdio client used by CLI, TUI, Desktop,
//! and protocol integration tests.

pub mod app_server;
mod app_server_events;
mod common;
mod message;

pub use app_server::{AppServerClient, AppServerClientConfig};
pub use message::{MessageCommandAck, MessageCommandStatus};
pub use talos_app_protocol::events::{canonical_method, ApplicationEvent};
pub use talos_app_protocol::requests::{
    AdminProjectListParams, AdminSessionListParams, AdminSessionPagination, EventSubscribeParams,
    EventSubscribeResponse, ProtocolTalosContext, SessionCreateParams, SessionHistoryParams,
    TimelineActiveTurnSnapshot, TimelineActiveTurnStatus, TimelineCursorStatus,
    TimelineRecoveryParams, TimelineRecoverySnapshot, TimelineReplay,
    ToolDynamicResultSubmitParams,
};
pub use talos_auth::{
    AdminCreateUserRequest, AdminPatchUserRequest, AuthResponse, LoginRequest, RegisterRequest,
    UserDto, UserRole,
};
pub use talos_config::LlmConfig;
pub use talos_error::{TalosError, TalosResult};
pub use talos_types::talos::{
    SessionHistoryMessage, SessionHistorySnapshot, StopReason, TalosContentItem, TalosContext,
    TalosInputItem, TalosItemCompletedEvent, TalosItemDeltaEvent, TalosItemStartedEvent,
    TalosJsonValue, TalosMessageRequest, TalosMessageResponse, TalosOutputItem, TalosProvider,
    TalosSourceType, TalosTimelineContent, TalosTimelineDelta, TalosTimelineEvent,
    TalosTimelineEventPayload, TalosTimelineEventType, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelineNoticeLevel, TalosTimelinePhase, TalosToolProgressEntry,
    TalosToolProgressKind, TalosToolResult, TalosTurnCompletedEvent, TalosTurnErrorEvent,
    TalosTurnStartedEvent, TalosUsage, TimelineEventEnvelope,
};
pub use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, MemoryFileDetail, MemoryFileSummary, ModelInfo,
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, ProjectPatch, ProjectRecord,
    SessionAgentConf, SessionConf, SessionQuery, TalosSession, WorkspaceDocDetail,
    WorkspaceDocKind, WorkspaceDocSummary,
};

pub async fn connect_app_server(config: AppServerClientConfig) -> TalosResult<AppServerClient> {
    AppServerClient::connect(config).await
}

pub async fn connect_app_server_from_env() -> TalosResult<AppServerClient> {
    AppServerClient::connect(AppServerClientConfig::from_env()?).await
}
