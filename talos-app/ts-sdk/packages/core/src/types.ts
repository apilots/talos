import type {
  TalosTimelineItem as ProtocolTalosTimelineItem,
  TalosTimelineEvent as ProtocolTalosTimelineEvent,
  TalosTimelineEventPayload as ProtocolTalosTimelineEventPayload,
  TalosTurnCompletedEvent as ProtocolTalosTurnCompletedEvent,
  TalosTurnMetricsEvent as ProtocolTalosTurnMetricsEvent,
  TimelineEventEnvelope as ProtocolTimelineEventEnvelope,
  TimelineRecoverySnapshot as ProtocolTimelineRecoverySnapshot,
  TimelineReplay as ProtocolTimelineReplay,
  TurnExecutionMetrics as ProtocolTurnExecutionMetrics,
  SessionHistoryMessage as ProtocolSessionHistoryMessage,
  SessionHistorySnapshot as ProtocolSessionHistorySnapshot,
  TalosConnectionMode as ProtocolTalosConnectionMode,
  TalosContentItem as ProtocolTalosContentItem,
  TalosContext as ProtocolTalosContext,
  TalosInputItem as ProtocolTalosInputItem,
  TalosMessageRequest as ProtocolTalosMessageRequest,
  TalosMessageRetryRequest as ProtocolTalosMessageRetryRequest,
  TalosProvider as ProtocolTalosProvider,
  TalosSessionEnvironment as ProtocolTalosSessionEnvironment,
  TalosSourceType as ProtocolTalosSourceType,
} from "./generated/app-protocol";

export type {
  TalosItemCompletedEvent,
  TalosItemDeltaEvent,
  TalosItemStartedEvent,
  TalosTimelineContent,
  TalosTimelineDelta,
  TalosTimelineEventType,
  TalosTimelineItem,
  TalosTimelineItemKind,
  TalosTimelineItemStatus,
  TalosTimelineNoticeLevel,
  TalosTimelinePhase,
  TimelineSubscriptionNotification,
  TimelineActiveTurnSnapshot,
  TimelineActiveTurnStatus,
  TimelineCursorStatus,
  TimelinePendingDynamicToolResult,
  TimelinePendingUserAction,
  TimelineRecoveryParams,
  SessionHistoryMessage,
  SessionHistoryParams,
  SessionHistorySnapshot,
  TalosToolProgressKind,
  TalosToolProgressEntry,
  TalosToolResult,
  TalosTurnErrorEvent,
  TalosTurnStartedEvent,
  TurnExecutionMetrics,
  FsDirectoryCreateParams,
  FsDirectoryCreateResponse,
  FsDirectoryEntry,
  FsDirectoryReadParams,
  FsDirectorySnapshot,
  FsEntryKind,
  ProtocolSkillDocument,
  ProtocolSkillLoadError,
  ProtocolSkillMetadata,
  ProtocolSkillScope,
  ProtocolSkillSelector,
  ProtocolSkillSuggestion,
  ProtocolSkillSummary,
  ProtocolSkillWarning,
  MarketplaceSource,
  MarketplaceSourceId,
  MarketplaceSourceInput,
  MarketplaceSourceKind,
  MarketplaceSourceLocation,
  MarketplaceSourceType,
  Plugin,
  PluginDefinition,
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
  SkillPlugin,
  SkillsListRequest,
  SkillsListResponse,
  SkillsReadRequest,
  SkillsRefreshRequest,
  SkillsResolveRequest,
  SkillsResolveResponse,
  SkillsSuggestRequest,
  SkillsSuggestResponse,
} from "./generated/app-protocol";
export type {
  AuthSessionReadResponse,
  AuthSessionSnapshot,
  AuthSessionStoreParams,
  AuthSessionUserSnapshot,
} from "./generated/app-protocol";

export type TalosConnectionMode = ProtocolTalosConnectionMode;

export type StopReason =
  | "end_turn"
  | "model_output_limit"
  | "tool_round_limit"
  | "transfer_unresolved"
  | "before_model_callback_stopped"
  | "after_model_callback_stopped"
  | "cancelled";

export type TalosSessionEnvironment = ProtocolTalosSessionEnvironment;
export type TalosSourceType = ProtocolTalosSourceType;
export type TalosProvider = ProtocolTalosProvider;
export type TalosContext = ProtocolTalosContext;

export type UserRole = "user" | "admin";

export interface Capabilities {
  is_admin: boolean;
  role: UserRole;
  scopes: string[];
}

export interface UserDto {
  user_id: string;
  username: string;
  email?: string | null;
  role: UserRole;
  disabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface TalosUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cache_creation_input_tokens?: number | null;
  cache_read_input_tokens?: number | null;
}

export interface TalosReasoning {
  text: string;
}

export type TalosInputItem = ProtocolTalosInputItem;

export type TalosContentItem = ProtocolTalosContentItem;

export type TalosOutputItem =
  | { type: "user_input"; text: string }
  | { type: "reasoning"; text: string }
  | { type: "execution_summary"; text: string }
  | { type: "output_text"; text: string }
  | {
      type: "tool_call";
      call_id: string;
      name: string;
      arguments: unknown;
    }
  | {
      type: "tool_result";
      call_id: string;
      tool_name: string;
      content: TalosContentItem[];
      is_error?: boolean;
    };

export type TalosMessageRequest = Omit<
  ProtocolTalosMessageRequest,
  "workspace_id" | "context" | "model" | "project_id" | "current_directory"
> & {
  workspace_id?: ProtocolTalosMessageRequest["workspace_id"];
  context?: ProtocolTalosMessageRequest["context"];
  model?: ProtocolTalosMessageRequest["model"];
  project_id?: ProtocolTalosMessageRequest["project_id"];
  current_directory?: ProtocolTalosMessageRequest["current_directory"];
};

export type TalosMessageRetryRequest = Omit<
  ProtocolTalosMessageRetryRequest,
  "workspace_id"
> & {
  workspace_id?: ProtocolTalosMessageRetryRequest["workspace_id"];
};

export interface TalosMessageResponse {
  request_id: string;
  message_id: string;
  session_id: string;
  model: string;
  output: TalosOutputItem[];
  reasoning: TalosReasoning[];
  stop_reason: StopReason;
  usage: TalosUsage;
}

export type TalosMessageCommandStatus =
  | "pending"
  | "accepted"
  | "failed"
  | "failed_no_first_event";

export interface TalosMessageAck {
  request_id: string;
  message_id?: string | null;
  session_id: string;
  turn_id: string;
  status: TalosMessageCommandStatus;
}

export type TalosTimelineEvent = Omit<
  ProtocolTalosTimelineEvent,
  "event_seq" | "created_at_ms" | "sequence" | "payload"
> & {
  event_seq?: number | null;
  created_at_ms?: number | null;
  sequence: number;
  payload: TalosTimelineEventPayload;
};

export type TalosTurnMetricsEvent = Omit<
  ProtocolTalosTurnMetricsEvent,
  "metrics"
> & {
  metrics: TalosTurnExecutionMetrics;
};

export type TalosTurnCompletedEvent = Omit<
  ProtocolTalosTurnCompletedEvent,
  "completed_at_ms" | "usage" | "metrics"
> & {
  completed_at_ms: number;
  usage: TalosUsage;
  metrics: TalosTurnExecutionMetrics | null;
};

export type TalosTimelineEventPayload =
  | Exclude<
      ProtocolTalosTimelineEventPayload,
      { type: "turn_metrics" } | { type: "turn_completed" }
    >
  | ({ type: "turn_metrics" } & TalosTurnMetricsEvent)
  | ({ type: "turn_completed" } & TalosTurnCompletedEvent);

export type TimelineEventEnvelope = Omit<
  ProtocolTimelineEventEnvelope,
  "event_seq" | "created_at_ms" | "event"
> & {
  event_seq: number;
  created_at_ms: number;
  event: TalosTimelineEvent;
};

export type TimelineReplay = Omit<
  ProtocolTimelineReplay,
  "events" | "last_replay_seq"
> & {
  events: TimelineEventEnvelope[];
  last_replay_seq: number;
};

export type TimelineRecoverySnapshot = Omit<
  ProtocolTimelineRecoverySnapshot,
  "replay" | "active_turn"
> & {
  replay: TimelineReplay;
  active_turn: TalosActiveTurn;
};

export type TalosHistoryTimelineItem = Omit<
  ProtocolTalosTimelineItem,
  "started_at_ms" | "completed_at_ms"
> & {
  started_at_ms: number;
  completed_at_ms?: number | null;
};

export type TalosTurnExecutionMetrics = Omit<
  ProtocolTurnExecutionMetrics,
  | "model_rounds"
  | "tool_calls"
  | "tool_successes"
  | "tool_errors"
  | "usage"
  | "duration_ms"
> & {
  model_rounds: number;
  tool_calls: number;
  tool_successes: number;
  tool_errors: number;
  usage: TalosUsage;
  duration_ms: number | null;
};

export type TalosSessionHistoryMessage = Omit<
  ProtocolSessionHistoryMessage,
  "items" | "usage" | "metrics"
> & {
  items: TalosHistoryTimelineItem[];
  usage: TalosUsage;
  metrics: TalosTurnExecutionMetrics | null;
};

export type TalosSessionHistorySnapshot = Omit<
  ProtocolSessionHistorySnapshot,
  "generated_at_ms" | "messages"
> & {
  generated_at_ms: number;
  messages: TalosSessionHistoryMessage[];
};

export type TalosActiveTurnStatus =
  | "running"
  | "waiting_user_action"
  | "cancelling"
  | "completed"
  | "failed";

export interface TalosPendingDynamicToolResult {
  type: "dynamic_tool_result";
  item_id?: string | null;
  call_id: string;
  tool_name: string;
}

export type TalosPendingUserAction = TalosPendingDynamicToolResult;

export interface TalosActiveTurn {
  session_id: string;
  turn_id?: string | null;
  message_id?: string | null;
  status: TalosActiveTurnStatus;
  last_event_id?: string | null;
  pending_user_action?: TalosPendingUserAction | null;
  updated_at: number;
}

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  context_window?: number;
  reasoning?: boolean;
  reasoning_effort?: ReasoningEffort;
  speed?: ModelSpeed;
  input?: string[];
  cost?: ModelCostConfig;
  compat?: ModelCompatConfig;
}

export interface SessionRecord {
  id: string;
  title: string;
  context: TalosContext;
  model: string;
  created_at: number;
  last_activity: number;
  message_count: number;
  usage: TalosUsage;
  conf: SessionConf;
  project_id?: string | null;
  current_directory?: string | null;
}

export interface TalosSession {
  title: string;
  user_id?: string;
  context: {
    session_id: string;
    workspace_id?: string | null;
    source_type: TalosSourceType;
    provider: TalosProvider;
    user_id: string;
    message_id: string;
    trace_id: string;
    channel_id?: string | null;
    thread_id?: string | null;
    project_id?: string | null;
    current_directory?: string | null;
    environment?: TalosSessionEnvironment | null;
  };
  conf: SessionConf;
  project_id?: string | null;
  current_directory?: string | null;
  config: {
    agent?: SessionAgentConf | null;
    llm?: SessionAgentConf | null;
  };
  state: {
    created_at: number;
    last_activity: number;
    message_count: number;
    usage: TalosUsage;
  };
}

export interface SessionQuery {
  user_id?: string;
  channel_id?: string;
  provider?: string;
  project_id?: string;
  limit?: number;
  offset?: number;
}

export interface PaginationMeta {
  total: number;
  limit: number;
  offset: number;
}

export interface PaginatedResponse<T> {
  data: T[];
  pagination: PaginationMeta;
}

export interface SessionAgentConf {
  model?: string | null;
}

export interface SessionConf {
  agent?: SessionAgentConf | null;
}

export type OptionalProjectIdPatch =
  | { type: "set"; project_id: string }
  | { type: "clear" };

export type OptionalCurrentDirectoryPatch =
  | { type: "set"; path: string }
  | { type: "clear" };

export interface SessionUpdate {
  conf?: SessionConf | null;
  project_id?: OptionalProjectIdPatch | null;
  current_directory?: OptionalCurrentDirectoryPatch | null;
}

export interface ProjectRecord {
  project_id: string;
  user_id?: string;
  name: string;
  directory: string;
  status?: ProjectStatus | null;
  created_at: number;
  updated_at: number;
  session_count: number;
}

export type ProjectStatusState = "available" | "missing" | "not_directory" | "permission_denied";
export type ProjectUnavailableReason = "path_not_found" | "not_directory" | "permission_denied";

export interface ProjectStatus {
  state: ProjectStatusState;
  reason?: ProjectUnavailableReason | null;
}

export interface ProjectPatch {
  name?: string | null;
}

export type ExpectedFileVersion =
  | { mode: "must_not_exist" }
  | { mode: "match"; version_token: string };

export interface DocUpdateRequest {
  content: string;
  base_version: ExpectedFileVersion;
}

export interface MemoryFileSummary {
  date: string;
  path: string;
  section_count: number;
  updated_at?: number | null;
  version_token: string;
}

export interface MemoryFileDetail extends MemoryFileSummary {
  content: string;
}

export interface MemoryFileListParams {
  limit?: number;
  before_date?: string;
  date_from?: string;
  date_to?: string;
}

export interface MemorySearchParams {
  q: string;
  limit?: number;
  date_from?: string;
  date_to?: string;
}

export interface MemorySearchResult {
  path: string;
  snippet: string;
  start_line: number;
  end_line: number;
}

export type WorkspaceDocKind = "soul" | "identity" | "user" | "heartbeat" | "memory";

export interface WorkspaceDocSummary {
  kind: WorkspaceDocKind;
  path: string;
  exists: boolean;
  updated_at?: number | null;
  version_token?: string | null;
}

export interface WorkspaceDocDetail extends WorkspaceDocSummary {
  content: string;
}

export interface AgentWorkspaceInfo {
  user_id: string;
  username?: string | null;
  base_workspace_dir: string;
  user_workspace_dir: string;
}

export type ModelInputKind = "text" | "image" | "audio" | "video" | "file";

export type ThinkingFormat = "open_ai" | "anthropic" | "qwen";

export interface ModelCompatConfig {
  thinking_format?: ThinkingFormat | null;
}

export interface ModelCostConfig {
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
}

export type ReasoningEffort = "low" | "medium" | "high" | "xhigh";
export type ModelSpeed = "standard" | "fast";

export interface ProviderModelConfig {
  id: string;
  name: string;
  context_window: number;
  reasoning: boolean;
  reasoning_effort?: ReasoningEffort;
  speed?: ModelSpeed;
  input: ModelInputKind[];
  cost: ModelCostConfig;
  compat: ModelCompatConfig;
}

export type LlmProtocol = "chat" | "responses" | "messages";

export interface ProviderConfig {
  api_key?: string | null;
  base_url?: string | null;
  enabled: boolean;
  protocol?: LlmProtocol | null;
  models: ProviderModelConfig[];
}

export interface LlmConfig {
  provider: string;
  model: string;
  anthropic: ProviderConfig;
  openai: ProviderConfig;
  bailian: ProviderConfig;
  ollama: ProviderConfig;
  openrouter: ProviderConfig;
}

export interface ModelReadiness {
  ready: boolean;
  provider: string;
  model: string;
  issues: string[];
}

export interface ProviderKeyStatus {
  api_key_configured: boolean;
}

export interface LlmConfigRead {
  config: LlmConfig;
  model_readiness: ModelReadiness;
  provider_keys: Record<string, ProviderKeyStatus>;
}
