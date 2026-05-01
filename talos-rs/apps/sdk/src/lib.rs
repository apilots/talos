//! Talos Rust SDK facade.
//!
//! The SDK exposes the local JSON-RPC stdio client used by CLI, TUI, Desktop,
//! and protocol integration tests.

pub mod app_server;
mod app_server_events;
mod common;

pub use app_server::{AppServerClient, AppServerClientConfig};
pub use talos_app_protocol::events::{
    canonical_method, ApplicationEvent, ApplicationEventEnvelope,
};
pub use talos_app_protocol::requests::{
    AdminProjectListParams, AdminSessionListParams, AdminSessionPagination, EventSubscribeParams,
    EventSubscribeResponse, ProtocolTalosContext, SessionCreateParams,
    ToolDynamicResultSubmitParams, TurnStartResponse,
};
pub use talos_auth::{AuthResponse, LoginRequest, RegisterRequest, UserRole};
pub use talos_error::{TalosError, TalosResult};
pub use talos_types::talos::{
    StopReason, TalosContentItem, TalosContext, TalosEvent, TalosEventPayload, TalosEventType,
    TalosInputItem, TalosJsonValue, TalosMessageCompletedEvent, TalosMessageErrorEvent,
    TalosMessageExecutionSummaryEvent, TalosMessageOutputDeltaEvent,
    TalosMessageReasoningDeltaEvent, TalosMessageRequest, TalosMessageResponse,
    TalosMessageStartedEvent, TalosMessageToolCallArgsDeltaEvent,
    TalosMessageToolCallCompletedEvent, TalosMessageToolCallExecutingEvent,
    TalosMessageToolCallPendingApprovalEvent, TalosMessageToolCallProgressDeltaEvent,
    TalosMessageToolCallQueuedEvent, TalosMessageToolCallStartedEvent, TalosOutputItem,
    TalosProvider, TalosSourceType, TalosThinkingSource, TalosThinkingStage, TalosToolProgressKind,
    TalosToolStreamKey, TalosUsage,
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
