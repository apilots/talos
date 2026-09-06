//! Talos Rust SDK facade.
//!
//! The SDK exposes the local JSON-RPC stdio client used by CLI, TUI, Desktop,
//! and protocol integration tests.

pub mod app_server;
mod contract;
mod login;

pub use app_server::{AppServerClient, AppServerClientConfig, AppServerOwnership};
pub use contract::{
    GatewayClient, MessageQueueChangeStream, TimelineEventStream, TimelineStreamSubscription,
};
pub use login::{now_ms, open_external_url};
pub use talos_agent_api::{
    ActiveTurnState, AgentAssistantText, AgentAssistantTextPhase, AgentContextCompaction,
    AgentContextCompactionDelta, AgentErrorInfo, AgentEvent, AgentEventPayload,
    AgentExecutionSummary, AgentInterruptOutcome, AgentMediaSource, AgentMessage,
    AgentMessageBlock, AgentMessageBlockCompleted, AgentMessageBlockContent,
    AgentMessageBlockDelta, AgentMessageBlockDeltaContent, AgentMessageBlockStarted,
    AgentMessageBlockStatus, AgentMessageRole, AgentMessageStarted, AgentMessageStatus,
    AgentPendingInput, AgentReasoning, AgentRunFailure, AgentRunMetrics, AgentRunOutput,
    AgentRunResult, AgentStopReason, AgentSystemNotice, AgentToolCall, AgentToolCallArguments,
    AgentToolCallArgumentsDelta, AgentToolProgress, AgentToolResult, AgentTurnCompleted,
    AgentTurnFailed, AgentTurnStarted, AgentUserContent, AgentUserInput, InterruptTurnRequest,
    InterruptTurnResponse, RecoverTimelineResponse, SessionActiveTurnResponse,
    SessionHistoryResponse, SessionHistoryTurn, SubmitTurnInput, SubmitTurnInputRequest,
    SubmitTurnInputResponse, TimelineActiveTurn, TimelineCursorStatus, TimelineEvent,
    TimelineReplay,
};
pub use talos_app_protocol::events::timeline_event_method;
pub use talos_auth::{
    AuthResponse, CloudLoginCancelRequest, CloudLoginClientKind, CloudLoginStartRequest,
    CloudLoginStartResponse, CloudLoginStatusRequest, CloudLoginStatusResponse, UserDto, UserRole,
};
pub use talos_error::{ApiError, ErrorCode, TalosError, TalosResult};
pub use talos_types::talos::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger, DynamicToolArguments, JsonContentValue, SessionSourceProvider,
    SessionSourceType, StopReason, TalosContentItem, TalosTimelineNoticeLevel,
    TalosToolProgressKind, TokenUsage, ToolArguments, ToolExecutionResult, TurnExecutionMetrics,
};
pub use talos_types::RecoverTimelineRequest;
pub use talos_types::{
    AgentPath, AgentTreeNode, AgentTreeSnapshot, AgentTurnRef, AuthLogoutRequest,
    AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse, AuthSessionReadRequest,
    AuthSessionReadResponse, AuthSessionSnapshot, AuthSessionUserSnapshot,
    CloudLoginCancelResponse, CollaborationToolResult, ConfigDeleteRequest, ConfigDeleteResponse,
    ConfigGetRequest, ConfigReloadRequest, ConfigReloadResponse, ConfigResponse,
    ConfigUpdateRequest, ConfigValue, ContextUsage, DocUpdateRequest, DynamicToolResult,
    EventUnsubscribeRequest, EventUnsubscribeResponse, ExpectedFileVersion, FileDeleteRequest,
    FileDeleteResponse, FileInfo, FileListRequest, FileListResponse, FileReadRequest,
    FileReadResponse, FileScope, FileUpdateRequest, FileUpdateResponse, FileUploadRequest,
    FileUploadResponse, FsDirectoryCreateRequest, FsDirectoryReadRequest, FsDirectoryReadResponse,
    LlmConfig, LlmProtocol, MemoryFileDetail, MemoryFileListRequest, MemoryFileListResponse,
    MemoryFileReadRequest, MemoryFileReadResponse, MemoryFileSummary, MemoryFileWriteRequest,
    MemoryFileWriteResponse, MemoryGetRequest, MemoryGetResponse, MemoryImportance, MemoryInfo,
    MemoryKind, MemorySearchHit, MemorySearchRequest, MemorySearchResponse, MemorySourceInfo,
    MessageQueue, MessageQueueAddRequest, MessageQueueChangedNotification,
    MessageQueueDeleteRequest, MessageQueueListRequest, MessageQueueReorderRequest,
    MessageQueueStartRequest, MessageQueueUpdateRequest, ModelCostConfig, ModelInfo,
    ModelInputKind, ModelListRequest, ModelListResponse, ModelReadiness, ModelRef, ModelSelection,
    ModelSpeed, OptionalCurrentDirectoryPatch, PendingUserAction, ProfileDailyUsage,
    ProfileModelUsage, ProfileReadRequest, ProfileReadResponse, ProjectCreateRequest,
    ProjectCreateResponse, ProjectDeleteRequest, ProjectDeleteResponse, ProjectInfo,
    ProjectListRequest, ProjectListResponse, ProjectPatch, ProjectPatchRequest,
    ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse, ProjectStatus,
    ProjectStatusState, ProjectUnavailableReason, ProviderConfig, ProviderKeyStatus,
    ProviderModelConfig, ProviderProtocolCapabilities, QuestionAnswer, QuestionOption,
    QuestionParams, QueuedMessage, ReasoningEffort, RetryTurnRequest, SandboxApprovalDecision,
    SandboxApprovalParams, SandboxApprovalPresentation, SandboxApprovalReason, SendMessageResult,
    SessionActiveTurnRequest, SessionClearRequest, SessionClearResponse, SessionCloseRequest,
    SessionCloseResponse, SessionCompactRequest, SessionCompactResponse, SessionConf,
    SessionDeleteRequest, SessionDeleteResponse, SessionGetRequest, SessionGetResponse,
    SessionHistoryRequest, SessionInfo, SessionListRequest, SessionListResponse,
    SessionPatchRequest, SessionUpdateResponse, SessionUsage, StartTurnRequest, StartTurnResponse,
    SubagentOutcome, SubagentRef, SubagentSnapshot, SubagentSpawnResult, SubagentUpdate,
    SubagentUpdateKind, SubscribeTimelineRequest, SubscribeTimelineResponse,
    TimelineUserActionEvent, TurnCommandStatus, UserActionParams, UserActionResponse,
    UserActionState, UserCurrentRequest, UserCurrentResponse, UserPermissionMode, WorkspaceDocInfo,
    WorkspaceDocKind, WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocReadResponse, WorkspaceDocSummary, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse,
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
