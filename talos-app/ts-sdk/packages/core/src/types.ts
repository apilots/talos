import type {
  DocUpdateRequest as ProtocolDocUpdateRequest,
  ExpectedFileVersion as ProtocolExpectedFileVersion,
  FileDeleteRequest as ProtocolFileDeleteRequest,
  FileDeleteResponse as ProtocolFileDeleteResponse,
  FileInfo as ProtocolFileInfo,
  FileListRequest as ProtocolFileListRequest,
  FileListResponse as ProtocolFileListResponse,
  FileReadRequest as ProtocolFileReadRequest,
  FileReadResponse as ProtocolFileReadResponse,
  FileScope as ProtocolFileScope,
  FileUpdateRequest as ProtocolFileUpdateRequest,
  FileUpdateResponse as ProtocolFileUpdateResponse,
  FileUploadRequest as ProtocolFileUploadRequest,
  FileUploadResponse as ProtocolFileUploadResponse,
  MemoryFileDetail as ProtocolMemoryFileDetail,
  MemoryFileSummary as ProtocolMemoryFileSummary,
  ConfigPath as ProtocolConfigPath,
  ConfigResponse as ProtocolConfigResponse,
  ConfigGetResponse as ProtocolConfigGetResponse,
  ConfigReloadResponse as ProtocolConfigReloadResponse,
  ConfigUpdateResponse as ProtocolConfigUpdateResponse,
  ConfigValue as ProtocolConfigValue,
  LlmConfig as ProtocolLlmConfig,
  LlmProtocol as ProtocolLlmProtocol,
  ModelCostConfig as ProtocolModelCostConfig,
  ModelInfo as ProtocolModelInfo,
  ModelInputKind as ProtocolModelInputKind,
  ModelListResponse as ProtocolModelListResponse,
  ModelReadiness as ProtocolModelReadiness,
  ModelRef as ProtocolModelRef,
  ModelSelection as ProtocolModelSelection,
  ModelSpeed as ProtocolModelSpeed,
  ProviderConfig as ProtocolProviderConfig,
  ProviderKeyStatus as ProtocolProviderKeyStatus,
  ProviderModelConfig as ProtocolProviderModelConfig,
  ProviderProtocolCapabilities as ProtocolProviderProtocolCapabilities,
  WebSearchToolConfig as ProtocolWebSearchToolConfig,
  WebSearchProviderConfig as ProtocolWebSearchProviderConfig,
  WebSearchProviderRuntimeConfig as ProtocolWebSearchProviderRuntimeConfig,
  ProfileDailyUsage as ProtocolProfileDailyUsage,
  ProfileModelUsage as ProtocolProfileModelUsage,
  ProfileReadRequest as ProtocolProfileReadRequest,
  ProfileReadResponse as ProtocolProfileReadResponse,
  ReasoningEffort as ProtocolReasoningEffort,
  SessionClearResponse as ProtocolSessionClearResponse,
  SessionCloseResponse as ProtocolSessionCloseResponse,
  SessionCompactResponse as ProtocolSessionCompactResponse,
  SessionDeleteResponse as ProtocolSessionDeleteResponse,
  SessionActiveTurnRequest as ProtocolSessionActiveTurnRequest,
  SessionClearRequest as ProtocolSessionClearRequest,
  SessionCloseRequest as ProtocolSessionCloseRequest,
  SessionCompactRequest as ProtocolSessionCompactRequest,
  SessionDeleteRequest as ProtocolSessionDeleteRequest,
  SessionGetRequest as ProtocolSessionGetRequest,
  SessionGetResponse as ProtocolSessionGetResponse,
  SessionInfo as ProtocolSessionInfo,
  SessionListRequest as ProtocolSessionListRequest,
  SessionListResponse as ProtocolSessionListResponse,
  SessionPatchRequest as ProtocolSessionPatchRequest,
  SessionUpdateResponse as ProtocolSessionUpdateResponse,
  SessionHistoryMessage as ProtocolSessionHistoryMessage,
  SessionHistorySnapshot as ProtocolSessionHistorySnapshot,
  StopReason as ProtocolStopReason,
  TalosConnectionMode as ProtocolTalosConnectionMode,
  TalosContentItem as ProtocolTalosContentItem,
  TalosContext as GeneratedTalosContext,
  TalosInputItem as ProtocolTalosInputItem,
  RunItem as ProtocolRunItem,
  TalosProvider as ProtocolTalosProvider,
  TalosReasoning as ProtocolTalosReasoning,
  TalosSessionEnvironment as ProtocolTalosSessionEnvironment,
  TalosSourceType as ProtocolTalosSourceType,
  AgentEvent as ProtocolAgentEvent,
  AgentEventPayload as ProtocolAgentEventPayload,
  AgentPath as ProtocolAgentPath,
  AgentTreeNode as ProtocolAgentTreeNode,
  AgentTreeSnapshot as ProtocolAgentTreeSnapshot,
  AgentTurnRef as ProtocolAgentTurnRef,
  CollaborationToolResult as ProtocolCollaborationToolResult,
  SendMessageResult as ProtocolSendMessageResult,
  SubagentOutcome as ProtocolSubagentOutcome,
  SubagentRef as ProtocolSubagentRef,
  SubagentSnapshot as ProtocolSubagentSnapshot,
  SubagentSpawnResult as ProtocolSubagentSpawnResult,
  SubagentUpdate as ProtocolSubagentUpdate,
  SubagentUpdateKind as ProtocolSubagentUpdateKind,
  TalosTimelineItem as ProtocolTalosTimelineItem,
  TalosTurnCompletedEvent as ProtocolTalosTurnCompletedEvent,
  TalosTurnMetricsEvent as ProtocolTalosTurnMetricsEvent,
  TokenUsage as ProtocolTokenUsage,
  TimelineActiveTurnSnapshot as ProtocolTimelineActiveTurnSnapshot,
  TimelineActiveTurnStatus as ProtocolTimelineActiveTurnStatus,
  TimelineEventEnvelope as ProtocolTimelineEventEnvelope,
  TimelinePendingUserAction as ProtocolTimelinePendingUserAction,
  TimelineRecoveryResponse as ProtocolTimelineRecoveryResponse,
  TimelineRecoverySnapshot as ProtocolTimelineRecoverySnapshot,
  TimelineReplay as ProtocolTimelineReplay,
  TurnExecutionMetrics as ProtocolTurnExecutionMetrics,
  UserDto as ProtocolUserDto,
  UserRole as ProtocolUserRole,
  WorkspaceDocInfo as ProtocolWorkspaceDocInfo,
  WorkspaceDocKind as ProtocolWorkspaceDocKind,
  WorkspaceDocSummary as ProtocolWorkspaceDocSummary,
} from "./generated/app-protocol";

export type {
  TalosItemCompletedEvent,
  TalosItemDeltaEvent,
  TalosItemStartedEvent,
  TalosTimelineContent,
  TalosTimelineDelta,
  AgentEventType,
  TalosTimelineItem,
  TalosTimelineItemKind,
  TalosTimelineItemStatus,
  TalosTimelineNoticeLevel,
  TalosTimelinePhase,
  TimelineSubscriptionNotification,
  SubscriptionTerminatedEvent,
  SubscriptionTerminatedReason,
  TimelineActiveTurnSnapshot,
  TimelineActiveTurnStatus,
  TimelineCursorStatus,
  TimelinePendingDynamicToolResult,
  TimelinePendingUserAction,
  RecoverTimelineRequest,
  SessionHistoryMessage,
  SessionActiveTurnResponse,
  SessionHistoryRequest,
  SessionHistoryResponse,
  SessionHistorySnapshot,
  TalosToolProgressKind,
  TalosToolProgressEntry,
  TalosToolResult,
  TalosTurnErrorEvent,
  TalosTurnStartedEvent,
  TurnExecutionMetrics,
  FsDirectoryCreateRequest,
  FsDirectoryCreateResponse,
  FsDirectoryEntry,
  FsDirectoryReadRequest,
  FsDirectorySnapshot,
  FsEntryKind,
  SkillDocument,
  SkillLoadError,
  SkillMetadata,
  SkillScope,
  SkillSelector,
  SkillSuggestion,
  SkillSummary,
  SkillWarning,
  MarketplaceSource,
  MarketplaceSourceId,
  MarketplaceSourceInput,
  MarketplaceSourceKind,
  MarketplaceSourceLocation,
  MarketplaceSourceType,
  AuthLogoutRequest,
  AuthLogoutResponse,
  AuthSessionClearRequest,
  AuthSessionClearResponse,
  AuthSessionReadRequest,
  AuthSessionReadResponse,
  AuthSessionSnapshot,
  AuthSessionUserSnapshot,
  CloudLoginCancelResponse,
  Plugin,
  PluginDefinition,
  PluginDocument,
  PluginDocumentMediaType,
  PluginDocumentRef,
  PluginDocumentRole,
  PluginDisableRequest,
  PluginDisableResponse,
  PluginEnableRequest,
  PluginEnableResponse,
  PluginInstall,
  PluginInstallConflictPolicy,
  PluginInstallId,
  PluginInstallRequest,
  PluginInstallResponse,
  PluginInstallSource,
  PluginInstallTarget,
  PluginListRequest,
  PluginListResponse,
  PluginListStatusFilter,
  PluginMetadata,
  PluginReadRequest,
  PluginReadResponse,
  PluginSearchRefreshPolicy,
  PluginSearchRequest,
  PluginSearchResponse,
  PluginSearchStatusFilter,
  PluginSourceAddRequest,
  PluginSourceAddResponse,
  PluginSourceListRequest,
  PluginSourceListResponse,
  PluginSourceRefreshRequest,
  PluginSourceRefreshResponse,
  PluginSourceRemoveRequest,
  PluginSourceRemoveResponse,
  PluginUpdateConflictPolicy,
  PluginUpdateRequest,
  PluginUpdateResponse,
  PluginUninstallRequest,
  PluginUninstallResponse,
  ProjectCreateRequest,
  ProjectCreateResponse,
  ProjectDeleteRequest,
  ProjectDeleteResponse,
  ProjectInfo,
  ProjectListRequest,
  ProjectListResponse,
  ProjectPatch,
  ProjectPatchRequest,
  ProjectPatchResponse,
  ProjectReadRequest,
  ProjectReadResponse,
  ProjectStatus,
  ProjectStatusState,
  ProjectUnavailableReason,
  DynamicToolArguments,
  InterruptTurnRequest,
  InterruptTurnResponse,
  JsonContentValue,
  McpCallToolRequest,
  McpCallToolResponse,
  McpContent,
  McpReadResourceRequest,
  McpReadResourceResponse,
  McpResourceContents,
  McpResourceDescriptor,
  McpResourceTemplateDescriptor,
  McpServerDefinition,
  McpServerRuntimeDescriptor,
  McpServerSnapshot,
  McpServerSource,
  McpServerStatus,
  McpToolDescriptor,
  McpTransportDefinition,
  McpTransportKind,
  ConfigDeleteRequest,
  ConfigDeleteResponse,
  ConfigGetRequest,
  ConfigReloadRequest,
  ConfigUpdateRequest,
  EventUnsubscribeRequest,
  EventUnsubscribeResponse,
  FsDirectoryReadResponse,
  MemoryFileListRequest,
  MemoryFileListResponse,
  MemoryFileReadRequest,
  MemoryFileReadResponse,
  MemoryFileWriteRequest,
  MemoryFileWriteResponse,
  MemoryGetRequest,
  MemoryGetResponse,
  MemoryImportance,
  MemoryInfo,
  MemoryKind,
  MemorySearchRequest,
  MemorySearchHit,
  MemorySourceInfo,
  ModelListRequest,
  SkillPlugin,
  RetryTurnRequest,
  RetryTurnResponse,
  StartTurnRequest,
  StartTurnResponse,
  SubmitDynamicToolResultRequest,
  SubmitDynamicToolResultResponse,
  SubscribeTimelineRequest,
  SubscribeTimelineResponse,
  SkillsListRequest,
  SkillsListResponse,
  SkillsReadRequest,
  SkillsReadResponse,
  SkillsRefreshRequest,
  SkillsResolveRequest,
  SkillsResolveResponse,
  SkillsSuggestRequest,
  SkillsSuggestResponse,
  ToolArguments,
  ToolExecutionResult,
  TurnCommandStatus,
  UserCurrentRequest,
  UserCurrentResponse,
  WorkspaceDocListRequest,
  WorkspaceDocListResponse,
  WorkspaceDocReadRequest,
  WorkspaceDocReadResponse,
  WorkspaceDocWriteRequest,
  WorkspaceDocWriteResponse,
  WorkspaceInfoRequest,
  WorkspaceInfoResponse,
  MemorySearchResponse,
} from "./generated/app-protocol";

export type TalosConnectionMode = ProtocolTalosConnectionMode;

export type StopReason = ProtocolStopReason;

export type TalosSessionEnvironment = ProtocolTalosSessionEnvironment;
export type TalosSourceType = ProtocolTalosSourceType;
export type TalosProvider = ProtocolTalosProvider;
export type TalosContext = GeneratedTalosContext;

export type UserRole = ProtocolUserRole;
export type UserDto = ProtocolUserDto;
export type TokenUsage = ProtocolTokenUsage;
export type ProfileReadRequest = ProtocolProfileReadRequest;
export type ProfileReadResponse = ProtocolProfileReadResponse;
export type ProfileDailyUsage = ProtocolProfileDailyUsage;
export type ProfileModelUsage = ProtocolProfileModelUsage;

export type TalosReasoning = ProtocolTalosReasoning;

export type TalosInputItem = ProtocolTalosInputItem;

export type TalosContentItem = ProtocolTalosContentItem;

export type AgentPath = ProtocolAgentPath;
export type AgentTurnRef = ProtocolAgentTurnRef;
export type SubagentRef = ProtocolSubagentRef;
export type SubagentSpawnResult = ProtocolSubagentSpawnResult;
export type SendMessageResult = ProtocolSendMessageResult;
export type SubagentOutcome = ProtocolSubagentOutcome;
export type SubagentUpdateKind = ProtocolSubagentUpdateKind;
export type SubagentUpdate = ProtocolSubagentUpdate;
export type SubagentSnapshot = ProtocolSubagentSnapshot;
export type AgentTreeNode = ProtocolAgentTreeNode;
export type AgentTreeSnapshot = ProtocolAgentTreeSnapshot;
export type CollaborationToolResult = ProtocolCollaborationToolResult;

export type RunItem = ProtocolRunItem;

export type AgentEvent = ProtocolAgentEvent;

export type TalosTurnMetricsEvent = ProtocolTalosTurnMetricsEvent;

export type TalosTurnCompletedEvent = ProtocolTalosTurnCompletedEvent;

export type AgentEventPayload = ProtocolAgentEventPayload;

export type TimelineEventEnvelope = ProtocolTimelineEventEnvelope;

export type TimelineReplay = ProtocolTimelineReplay;

export type TimelineRecoverySnapshot = ProtocolTimelineRecoverySnapshot;

export type TimelineRecoveryResponse = ProtocolTimelineRecoveryResponse;

export type TalosHistoryTimelineItem = ProtocolTalosTimelineItem;

export type TalosTurnExecutionMetrics = ProtocolTurnExecutionMetrics;

export type TalosSessionHistoryMessage = ProtocolSessionHistoryMessage;

export type TalosSessionHistorySnapshot = ProtocolSessionHistorySnapshot;

export type TalosActiveTurnStatus = ProtocolTimelineActiveTurnStatus;
export type TalosPendingUserAction = ProtocolTimelinePendingUserAction;
export type TalosActiveTurn = ProtocolTimelineActiveTurnSnapshot;

export type ModelInfo = ProtocolModelInfo;
export type ModelRef = ProtocolModelRef;
export type ModelSelection = ProtocolModelSelection;
export type ModelListResponse = ProtocolModelListResponse;
export type SessionInfo = ProtocolSessionInfo;
export type SessionListRequest = ProtocolSessionListRequest;
export type SessionListResponse = ProtocolSessionListResponse;
export type SessionGetRequest = ProtocolSessionGetRequest;
export type SessionGetResponse = ProtocolSessionGetResponse;
export type SessionPatchRequest = ProtocolSessionPatchRequest;
export type SessionUpdateResponse = ProtocolSessionUpdateResponse;
export type SessionDeleteRequest = ProtocolSessionDeleteRequest;
export type SessionDeleteResponse = ProtocolSessionDeleteResponse;
export type SessionCloseRequest = ProtocolSessionCloseRequest;
export type SessionCloseResponse = ProtocolSessionCloseResponse;
export type SessionCompactRequest = ProtocolSessionCompactRequest;
export type SessionCompactResponse = ProtocolSessionCompactResponse;
export type SessionClearRequest = ProtocolSessionClearRequest;
export type SessionClearResponse = ProtocolSessionClearResponse;
export type SessionActiveTurnRequest = ProtocolSessionActiveTurnRequest;

export interface PaginationMeta {
  total: number;
  limit: number;
  offset: number;
}

export interface PaginatedResponse<T> {
  data: T[];
  pagination: PaginationMeta;
}

export type SessionConf = import("./generated/app-protocol").SessionConf;
export type OptionalProjectIdPatch = import("./generated/app-protocol").OptionalProjectIdPatch;
export type OptionalCurrentDirectoryPatch = import("./generated/app-protocol").OptionalCurrentDirectoryPatch;

export type ExpectedFileVersion = ProtocolExpectedFileVersion;
export type DocUpdateRequest = ProtocolDocUpdateRequest;
export type MemoryFileSummary = ProtocolMemoryFileSummary;
export type MemoryFileDetail = ProtocolMemoryFileDetail;

export type FileScope = ProtocolFileScope;
export type FileInfo = ProtocolFileInfo;
export type FileUploadRequest = ProtocolFileUploadRequest;
export type FileUploadResponse = ProtocolFileUploadResponse;
export type FileListRequest = ProtocolFileListRequest;
export type FileListResponse = ProtocolFileListResponse;
export type FileReadRequest = ProtocolFileReadRequest;
export type FileReadResponse = ProtocolFileReadResponse;
export type FileUpdateRequest = ProtocolFileUpdateRequest;
export type FileUpdateResponse = ProtocolFileUpdateResponse;
export type FileDeleteRequest = ProtocolFileDeleteRequest;
export type FileDeleteResponse = ProtocolFileDeleteResponse;
export type FileUploadData = Blob | Uint8Array;

export type WorkspaceDocKind = ProtocolWorkspaceDocKind;
export type WorkspaceDocSummary = ProtocolWorkspaceDocSummary;
export type WorkspaceDocInfo = ProtocolWorkspaceDocInfo;
export type ModelInputKind = ProtocolModelInputKind;
export type ModelCostConfig = ProtocolModelCostConfig;
export type ReasoningEffort = ProtocolReasoningEffort;
export type ModelSpeed = ProtocolModelSpeed;
export type ProviderModelConfig = ProtocolProviderModelConfig;
export type LlmProtocol = ProtocolLlmProtocol;
export type ProviderConfig = ProtocolProviderConfig;
export type LlmConfig = ProtocolLlmConfig;
export type ModelReadiness = ProtocolModelReadiness;
export type ProviderKeyStatus = ProtocolProviderKeyStatus;
export type ProviderProtocolCapabilities = ProtocolProviderProtocolCapabilities;
export type ConfigPath = ProtocolConfigPath;
export type ConfigValue = ProtocolConfigValue;
export type ConfigResponse = ProtocolConfigResponse;
export type ConfigGetResponse = ProtocolConfigGetResponse;
export type ConfigUpdateResponse = ProtocolConfigUpdateResponse;
export type ConfigReloadResponse = ProtocolConfigReloadResponse;
export type WebSearchToolConfig = ProtocolWebSearchToolConfig;
export type WebSearchProviderConfig = ProtocolWebSearchProviderConfig;
export type WebSearchProviderRuntimeConfig = ProtocolWebSearchProviderRuntimeConfig;
