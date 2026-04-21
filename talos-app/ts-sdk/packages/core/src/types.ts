export type TalosConnectionMode = "embedded" | "remote";

export type StopReason =
  | "end_turn"
  | "tool_round_limit"
  | "transfer_unresolved"
  | "before_model_callback_stopped"
  | "after_model_callback_stopped"
  | "cancelled";

export interface TalosSessionEnvironment {
  directory: string;
  connection_mode?: TalosConnectionMode;
}

export type TalosSourceType = "http" | "im" | "cli" | "ws" | "system";
export type TalosProvider =
  | "http"
  | "sdk"
  | "cli"
  | "telegram"
  | "websocket"
  | "system";

export interface TalosContext {
  source_type: TalosSourceType;
  provider: TalosProvider;
  user_id: string;
  message_id: string;
  trace_id: string;
  channel_id?: string | null;
  thread_id?: string | null;
  environment?: TalosSessionEnvironment | null;
}

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

export interface AuthResponse {
  access_token: string;
  token_type: string;
  expires_at: number;
  user: UserDto;
  capabilities: Capabilities;
}

export interface RegisterRequest {
  username: string;
  password: string;
  email?: string | null;
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface UpdateCurrentUserRequest {
  username?: string;
  email?: string | null;
}

export interface AdminCreateUserRequest {
  username: string;
  password: string;
  email?: string | null;
  role: UserRole;
  disabled?: boolean;
}

export interface AdminPatchUserRequest {
  username?: string;
  email?: string | null;
  password?: string;
  role?: UserRole;
  disabled?: boolean;
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

export type TalosInputItem =
  | { type: "text"; text: string }
  | { type: "image"; url: string; mime_type?: string | null }
  | {
      type: "file";
      url?: string | null;
      file_id?: string | null;
      mime_type?: string | null;
    }
  | {
      type: "voice";
      url?: string | null;
      data_base64?: string | null;
      mime_type?: string | null;
    }
  | { type: "video"; url?: string | null; mime_type?: string | null };

export type TalosContentItem =
  | { type: "text"; text: string }
  | { type: "json"; value: unknown };

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

export interface TalosMessageRequest {
  input: TalosInputItem[];
  stream?: boolean | null;
  workspace_id?: string | null;
}

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

export type TalosEventType =
  | "message_started"
  | "message_reasoning_delta"
  | "message_execution_summary"
  | "message_output_delta"
  | "message_tool_call_started"
  | "message_tool_call_args_delta"
  | "message_tool_call_queued"
  | "message_tool_call_executing"
  | "message_tool_call_progress_delta"
  | "message_tool_call_completed"
  | "message_tool_call_pending_approval"
  | "message_completed"
  | "message_error";

export type TalosThinkingSource = "provider" | "framework";
export type TalosThinkingStage =
  | "model_reasoning"
  | "framework_plan"
  | "framework_act"
  | "framework_observe"
  | "framework_reflect";

export interface TalosMessageStartedEvent {
  request_id: string;
  model: string;
  usage: TalosUsage;
  iteration: number;
  step_id: string;
}

export interface TalosMessageReasoningDeltaEvent {
  request_id: string;
  delta: string;
  source: TalosThinkingSource;
  stage: TalosThinkingStage;
  item_index: number;
  iteration: number;
  step_id: string;
}

export interface TalosMessageExecutionSummaryEvent {
  request_id: string;
  text: string;
  item_index: number;
  iteration: number;
  step_id: string;
}

export interface TalosMessageOutputDeltaEvent {
  request_id: string;
  delta: string;
  item_index: number;
  iteration: number;
  step_id: string;
}

export interface TalosMessageToolCallStartedEvent {
  request_id: string;
  call_id: string;
  name: string;
  arguments: unknown;
  item_index: number;
  iteration: number;
  step_id: string;
  timing_ms?: number | null;
}

export interface TalosToolStreamKey {
  round: number;
  index: number;
}

export type TalosToolProgressKind = "status" | "stdout" | "stderr";

export interface TalosMessageToolCallArgsDeltaEvent {
  request_id: string;
  stream_key: TalosToolStreamKey;
  call_id?: string | null;
  tool_name?: string | null;
  delta: string;
  item_index: number;
  iteration: number;
  step_id: string;
}

export interface TalosMessageToolCallQueuedEvent {
  request_id: string;
  stream_key: TalosToolStreamKey;
  call_id: string;
  tool_name: string;
  item_index: number;
  iteration: number;
  step_id: string;
}

export type TalosMessageToolCallExecutingEvent = TalosMessageToolCallQueuedEvent;

export interface TalosMessageToolCallProgressDeltaEvent {
  request_id: string;
  stream_key: TalosToolStreamKey;
  call_id: string;
  tool_name: string;
  message: string;
  progress_kind: TalosToolProgressKind;
  item_index: number;
  iteration: number;
  step_id: string;
}

export interface TalosMessageToolCallCompletedEvent {
  request_id: string;
  call_id: string;
  tool: string;
  content: TalosContentItem[];
  is_error: boolean;
  item_index: number;
  iteration: number;
  step_id: string;
  timing_ms?: number | null;
}

export interface TalosMessageToolCallPendingApprovalEvent {
  request_id: string;
  call_id: string;
  tool_name: string;
  prompt: string;
  arguments: unknown;
  step_id: string;
}

export interface TalosMessageCompletedEvent {
  request_id: string;
  stop_reason: StopReason;
  usage: TalosUsage;
  total_output_items: number;
  iteration: number;
  step_id: string;
  task_count: number;
}

export interface TalosMessageErrorEvent {
  request_id: string;
  error: string;
}

export type TalosEventPayload =
  | { type: "message_started"; data: TalosMessageStartedEvent }
  | { type: "message_reasoning_delta"; data: TalosMessageReasoningDeltaEvent }
  | { type: "message_execution_summary"; data: TalosMessageExecutionSummaryEvent }
  | { type: "message_output_delta"; data: TalosMessageOutputDeltaEvent }
  | { type: "message_tool_call_started"; data: TalosMessageToolCallStartedEvent }
  | { type: "message_tool_call_args_delta"; data: TalosMessageToolCallArgsDeltaEvent }
  | { type: "message_tool_call_queued"; data: TalosMessageToolCallQueuedEvent }
  | { type: "message_tool_call_executing"; data: TalosMessageToolCallExecutingEvent }
  | { type: "message_tool_call_progress_delta"; data: TalosMessageToolCallProgressDeltaEvent }
  | { type: "message_tool_call_completed"; data: TalosMessageToolCallCompletedEvent }
  | {
      type: "message_tool_call_pending_approval";
      data: TalosMessageToolCallPendingApprovalEvent;
    }
  | { type: "message_completed"; data: TalosMessageCompletedEvent }
  | { type: "message_error"; data: TalosMessageErrorEvent };

export interface TalosEvent {
  event_type: TalosEventType;
  message_id: string;
  session_id: string;
  sequence: number;
  payload: {
    type: TalosEventType;
  } & Record<string, unknown>;
}

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  context_window?: number;
  max_tokens?: number;
  reasoning?: boolean;
  input?: string[];
  cost?: Record<string, unknown>;
  compat?: Record<string, unknown>;
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
  owner_user_id?: string;
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
    agent?: { model?: string } & Record<string, unknown>;
    llm?: { model?: string } & Record<string, unknown>;
  } & Record<string, unknown>;
  state: {
    created_at: number;
    last_activity: number;
    message_count: number;
    usage: TalosUsage;
  };
}

export interface SessionQuery {
  user_id?: string;
  owner_user_id?: string;
  channel_id?: string;
  provider?: string;
  project_id?: string;
}

export interface SessionAgentConf {
  model?: string | null;
}

export type ToolApprovalPolicy = "on_request" | "always" | "never";

export interface SessionToolsConf {
  approval_policy?: ToolApprovalPolicy | null;
}

export interface SessionConf {
  agent?: SessionAgentConf | null;
  tools?: SessionToolsConf | null;
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
  owner_user_id?: string;
  name: string;
  directory: string;
  created_at: number;
  updated_at: number;
  session_count: number;
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

export interface ToolApprovalRequest {
  call_id: string;
  tool_name: string;
  approved: boolean;
  reason?: string | null;
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

export interface ProviderModelConfig {
  id: string;
  name: string;
  context_window: number;
  max_tokens: number;
  reasoning: boolean;
  input: ModelInputKind[];
  cost: ModelCostConfig;
  compat: ModelCompatConfig;
}

export interface ProviderConfig {
  api_key?: string | null;
  base_url?: string | null;
  enabled: boolean;
  models: ProviderModelConfig[];
}

export interface LlmConfig {
  provider: string;
  model: string;
  anthropic: ProviderConfig;
  openai: ProviderConfig;
  bailian: ProviderConfig;
  ollama: ProviderConfig;
}
