//! Talos Rust SDK facade.
//!
//! The SDK exposes the local JSON-RPC stdio client used by CLI, TUI, Desktop,
//! and protocol integration tests.

pub mod app_server;
mod contract;
mod login;

pub use app_server::{AppServerClient, AppServerClientConfig, AppServerOwnership};
pub use contract::{GatewayClient, TimelineEventStream, TimelineStreamSubscription};
pub use login::{now_ms, open_external_url};
pub use talos_app_protocol::events::{canonical_method, ApplicationEvent};
pub use talos_app_protocol::requests::{TimelineActiveTurnSnapshot, TimelineActiveTurnStatus};
pub use talos_auth::{
    AuthResponse, CloudLoginCancelRequest, CloudLoginClientKind, CloudLoginStartRequest,
    CloudLoginStartResponse, CloudLoginStatusRequest, CloudLoginStatusResponse, UserDto, UserRole,
};
pub use talos_error::{TalosError, TalosResult};
pub use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, ContextCompactionMetrics,
    ContextCompactionOutcome, ContextCompactionStage, ContextCompactionTrigger,
    DynamicToolArguments, JsonContentValue, RunItem, SessionHistoryMessage, SessionHistorySnapshot,
    StopReason, TalosAssistantTextCommittedEvent, TalosContentItem, TalosContext, TalosInputItem,
    TalosItemCompletedEvent, TalosItemDeltaEvent, TalosItemStartedEvent, TalosProvider,
    TalosSourceType, TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem,
    TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelineNoticeLevel, TalosTimelinePhase,
    TalosToolProgressEntry, TalosToolProgressKind, TalosToolResult, TalosTurnCompletedEvent,
    TalosTurnErrorEvent, TalosTurnMetricsEvent, TalosTurnStartedEvent, TimelineEventEnvelope,
    TokenUsage, ToolArguments, ToolExecutionResult, TurnExecutionMetrics,
};
pub use talos_types::RecoverTimelineRequest;
pub use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, AuthSessionSnapshot, AuthSessionUserSnapshot,
    CloudLoginCancelResponse, ConfigDeleteRequest, ConfigDeleteResponse, ConfigGetRequest,
    ConfigGetResponse, ConfigReloadRequest, ConfigReloadResponse, ConfigUpdateRequest,
    ConfigUpdateResponse, ConfigValue, DocUpdateRequest, EventUnsubscribeRequest,
    EventUnsubscribeResponse, ExpectedFileVersion, FsDirectoryCreateRequest,
    FsDirectoryReadRequest, FsDirectoryReadResponse, InterruptTurnRequest, InterruptTurnResponse,
    LlmConfig, LlmModelCompat, LlmModelConfig, LlmModelCost, LlmModelInputKind, LlmModelSpeed,
    LlmProtocol, LlmProviderConfig, LlmReasoningEffort, LlmThinkingFormat, MemoryFileDetail,
    MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest, MemoryFileReadResponse,
    MemoryFileSummary, MemoryFileWriteRequest, MemoryFileWriteResponse, MemorySearchRequest,
    ModelInfo, ModelListRequest, ModelListResponse, ModelReadiness, ModelRef,
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, ProjectCreateRequest,
    ProjectCreateResponse, ProjectDeleteRequest, ProjectDeleteResponse, ProjectInfo,
    ProjectListRequest, ProjectListResponse, ProjectPatch, ProjectPatchRequest,
    ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse, ProjectStatus,
    ProjectStatusState, ProjectUnavailableReason, ProviderKeyStatus, RetryTurnRequest,
    RetryTurnResponse, SessionActiveTurnRequest, SessionActiveTurnResponse, SessionClearRequest,
    SessionClearResponse, SessionCloseRequest, SessionCloseResponse, SessionCompactRequest,
    SessionCompactResponse, SessionConf, SessionDeleteRequest, SessionDeleteResponse,
    SessionGetRequest, SessionGetResponse, SessionHistoryRequest, SessionHistoryResponse,
    SessionInfo, SessionListRequest, SessionListResponse, SessionPatchRequest,
    SessionUpdateResponse, StartTurnRequest, StartTurnResponse, SubmitDynamicToolResultRequest,
    SubmitDynamicToolResultResponse, SubscribeTimelineRequest, SubscribeTimelineResponse,
    TimelineCursorStatus, TimelineRecoveryResponse, TimelineReplay, TurnCommandStatus,
    UserCurrentRequest, UserCurrentResponse, WorkspaceDocInfo, WorkspaceDocKind,
    WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocReadResponse, WorkspaceDocSummary, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse, WorkspaceInfoRequest, WorkspaceInfoResponse,
};
pub use talos_types::{
    MarketplaceSource, MarketplaceSourceInput, Plugin, PluginDefinition, PluginDisableRequest,
    PluginDisableResponse, PluginDocument, PluginDocumentMediaType, PluginDocumentRef,
    PluginDocumentRole, PluginEnableRequest, PluginEnableResponse, PluginInstall,
    PluginInstallConflictPolicy, PluginInstallId, PluginInstallRequest, PluginInstallResponse,
    PluginInstallSource, PluginInstallTarget, PluginKind, PluginListRequest, PluginListResponse,
    PluginListStatusFilter, PluginMetadata, PluginReadRequest, PluginReadResponse,
    PluginSearchRefreshPolicy, PluginSearchRequest, PluginSearchResponse, PluginSearchStatusFilter,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginStatus, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateConflictPolicy, PluginUpdateRequest, PluginUpdateResponse,
    SkillPlugin,
};
pub use talos_types::{
    SkillDocument, SkillLoadError, SkillMetadata, SkillScope, SkillSelector, SkillSuggestion,
    SkillSummary, SkillWarning, SkillsListRequest, SkillsListResponse, SkillsReadRequest,
    SkillsReadResponse, SkillsRefreshRequest, SkillsResolveRequest, SkillsResolveResponse,
    SkillsSuggestRequest, SkillsSuggestResponse,
};

pub async fn connect_app_server(config: AppServerClientConfig) -> TalosResult<AppServerClient> {
    AppServerClient::connect(config).await
}

pub async fn connect_app_server_from_env() -> TalosResult<AppServerClient> {
    AppServerClient::connect(AppServerClientConfig::from_env()?).await
}
