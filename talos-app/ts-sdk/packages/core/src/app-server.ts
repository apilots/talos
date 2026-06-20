import type * as AppProtocol from "./generated/app-protocol";
import type {
  MemoryFileDetail,
  MemoryFileSummary,
  MemorySearchResult,
  FsDirectoryCreateResponse,
  FsDirectorySnapshot,
  LlmConfig,
  LlmConfigRead,
  ModelInfo,
  ProjectPatch,
  ProjectRecord,
  TalosActiveTurn,
  TalosMessageAck,
  TalosSessionHistorySnapshot,
  TalosSession,
  TalosTurnExecutionMetrics,
  TalosUsage,
  TimelineRecoverySnapshot,
  UserDto,
  AuthSessionReadResponse,
  AuthSessionStoreParams,
  AgentWorkspaceInfo,
  WorkspaceDocDetail,
  WorkspaceDocKind,
  WorkspaceDocSummary,
} from "./types";

export type AppServerEventHandler = (
  event: AppProtocol.TimelineEventEnvelope,
) => void;

export type AppServerSubscriptionTerminatedHandler = (
  envelope: AppProtocol.SubscriptionTerminatedEnvelope,
) => void;

export type AppServerErrorHandler = (error: Error) => void;

export interface AppServerRequestOptions {
  actorCredentials?: AppProtocol.ActorCredentials | null;
}

export interface AppServerTransport {
  request<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: AppServerRequestOptions,
  ): Promise<T>;
  onEvent(handler: AppServerEventHandler): () => void;
  onSubscriptionTerminated?(handler: AppServerSubscriptionTerminatedHandler): () => void;
  onError?(handler: AppServerErrorHandler): () => void;
}

interface ListResponse<T> {
  data: T[];
}

export class AppServerClient {
  constructor(protected readonly transport: AppServerTransport) {}

  onEvent(handler: AppServerEventHandler): () => void {
    return this.transport.onEvent(handler);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    return this.transport.onSubscriptionTerminated?.(handler) ?? (() => undefined);
  }

  onError(handler: AppServerErrorHandler): () => void {
    return this.transport.onError?.(handler) ?? (() => undefined);
  }

  listModels(): Promise<ModelInfo[]> {
    return this.transport
      .request<ListResponse<ModelInfo>>("model.list", {})
      .then((response) => response.data);
  }

  configRead(): Promise<LlmConfigRead> {
    return this.transport.request("config.read", {});
  }

  configWriteValue(path: string, value: string): Promise<LlmConfigRead> {
    return this.transport.request("config.value.write", { path, value });
  }

  getLlmConfig(): Promise<LlmConfigRead> {
    return this.configRead();
  }

  updateLlmConfig(config: LlmConfig): Promise<LlmConfigRead> {
    return this.configWriteValue("llm", JSON.stringify(config));
  }

  configWriteBatch(entries: AppProtocol.ConfigValueWriteParams[]): Promise<unknown> {
    return this.transport.request("config.batch.write", { entries });
  }

  configReload(): Promise<unknown> {
    return this.transport.request("config.reload", {});
  }

  authLogout(): Promise<unknown> {
    return this.transport.request("auth.logout", {});
  }

  startCloudLogin(params: AppProtocol.CloudLoginStartRequest): Promise<AppProtocol.CloudLoginStartResponse> {
    return this.transport.request("auth.login.start", params);
  }

  getCloudLoginStatus(params: AppProtocol.CloudLoginStatusRequest): Promise<AppProtocol.CloudLoginStatusResponse> {
    return this.transport.request("auth.login.status", params);
  }

  cancelCloudLogin(params: AppProtocol.CloudLoginCancelRequest): Promise<unknown> {
    return this.transport.request("auth.login.cancel", params);
  }

  readAuthSession(): Promise<AuthSessionReadResponse> {
    return this.transport.request("auth.session.read", {});
  }

  storeAuthSession(params: AuthSessionStoreParams): Promise<unknown> {
    return this.transport.request("auth.session.store", params);
  }

  clearAuthSession(): Promise<unknown> {
    return this.transport.request("auth.session.clear", {});
  }

  getCurrentUser(): Promise<UserDto> {
    return this.transport.request("user.current", {});
  }

  listPluginSources(
    params: AppProtocol.PluginSourceListRequest = {},
  ): Promise<AppProtocol.PluginSourceListResponse> {
    return this.transport.request("plugin.source.list", params);
  }

  addPluginSource(params: AppProtocol.PluginSourceAddRequest): Promise<AppProtocol.PluginSourceAddResponse> {
    return this.transport.request("plugin.source.add", params);
  }

  refreshPluginSource(
    params: AppProtocol.PluginSourceRefreshRequest,
  ): Promise<AppProtocol.PluginSourceRefreshResponse> {
    return this.transport.request("plugin.source.refresh", params);
  }

  removePluginSource(
    params: AppProtocol.PluginSourceRemoveRequest,
  ): Promise<AppProtocol.PluginSourceRemoveResponse> {
    return this.transport.request("plugin.source.remove", params);
  }

  searchPlugins(params: AppProtocol.PluginSearchRequest): Promise<AppProtocol.PluginSearchResponse> {
    return this.transport.request("plugin.search", params);
  }

  listPlugins(params: AppProtocol.PluginListRequest): Promise<AppProtocol.PluginListResponse> {
    return this.transport.request("plugin.list", params);
  }

  readPlugin(params: AppProtocol.PluginReadRequest): Promise<AppProtocol.PluginReadResponse> {
    return this.transport.request("plugin.read", params);
  }

  installPlugin(params: AppProtocol.PluginInstallRequest): Promise<AppProtocol.PluginInstallResponse> {
    return this.transport.request("plugin.install", params);
  }

  updatePlugin(params: AppProtocol.PluginUpdateRequest): Promise<AppProtocol.PluginUpdateResponse> {
    return this.transport.request("plugin.update", params);
  }

  uninstallPlugin(params: AppProtocol.PluginUninstallRequest): Promise<AppProtocol.PluginUninstallResponse> {
    return this.transport.request("plugin.uninstall", params);
  }

  enablePlugin(params: AppProtocol.PluginEnableRequest): Promise<AppProtocol.PluginEnableResponse> {
    return this.transport.request("plugin.enable", params);
  }

  disablePlugin(params: AppProtocol.PluginDisableRequest): Promise<AppProtocol.PluginDisableResponse> {
    return this.transport.request("plugin.disable", params);
  }

  listSkills(params: AppProtocol.SkillsListRequest = { include_disabled: false }): Promise<AppProtocol.SkillsListResponse> {
    return this.transport.request("skills.list", params);
  }

  refreshSkills(params: AppProtocol.SkillsRefreshRequest = {}): Promise<AppProtocol.SkillsListResponse> {
    return this.transport.request("skills.refresh", params);
  }

  suggestSkills(params: AppProtocol.SkillsSuggestRequest): Promise<AppProtocol.SkillsSuggestResponse> {
    return this.transport.request("skills.suggest", params);
  }

  readSkill(params: AppProtocol.SkillsReadRequest): Promise<AppProtocol.SkillDocument> {
    return this.transport.request("skills.read", params);
  }

  resolveSkills(params: AppProtocol.SkillsResolveRequest): Promise<AppProtocol.SkillsResolveResponse> {
    return this.transport.request("skills.resolve", params);
  }

  listApps(): Promise<unknown[]> {
    return this.transport
      .request<ListResponse<unknown>>("app.list", {})
      .then((response) => response.data);
  }

  listTools(): Promise<unknown[]> {
    return this.transport
      .request<ListResponse<unknown>>("tool.list", {})
      .then((response) => response.data);
  }

  listSessions(params: AppProtocol.SessionListParams): Promise<TalosSession[]> {
    return this.transport
      .request<ListResponse<TalosSession>>("session.list", params)
      .then((response) => response.data);
  }

  getSession(sessionId: string): Promise<TalosSession> {
    return this.transport.request("session.read", { session_id: sessionId });
  }

  patchSession(params: AppProtocol.SessionPatchParams): Promise<unknown> {
    return this.transport.request("session.patch", params);
  }

  deleteSession(sessionId: string): Promise<unknown> {
    return this.transport.request("session.delete", { session_id: sessionId });
  }

  closeSession(sessionId: string): Promise<unknown> {
    return this.transport.request("session.close", { session_id: sessionId });
  }

  compactSession(sessionId: string): Promise<TalosSession> {
    return this.transport.request("session.compact", { session_id: sessionId });
  }

  clearSession(sessionId: string): Promise<TalosSession> {
    return this.transport.request("session.clear", { session_id: sessionId });
  }

  getActiveTurn(sessionId: string): Promise<TalosActiveTurn> {
    return this.transport.request("session.active_turn", { session_id: sessionId });
  }

  getSessionHistory(sessionId: string): Promise<TalosSessionHistorySnapshot> {
    return this.transport
      .request<TalosSessionHistorySnapshot>("session.history", { session_id: sessionId })
      .then(normalizeSessionHistorySnapshot);
  }

  recoverSession(
    sessionId: string,
    afterEventId: string | null = null,
    replayLimit: number | null = null,
  ): Promise<TimelineRecoverySnapshot> {
    return this.transport
      .request<TimelineRecoverySnapshot>("session.recover", {
        session_id: sessionId,
        after_event_id: afterEventId,
        replay_limit: replayLimit,
      })
      .then(normalizeTimelineRecoverySnapshot);
  }

  listProjects(): Promise<ProjectRecord[]> {
    return this.transport
      .request<ListResponse<ProjectRecord>>("project.list", {})
      .then((response) => response.data);
  }

  getProject(projectId: string): Promise<ProjectRecord> {
    return this.transport.request("project.read", { project_id: projectId });
  }

  createProject(name: string, directory: string): Promise<ProjectRecord> {
    return this.transport.request("project.create", { name, directory });
  }

  patchProject(projectId: string, patch: ProjectPatch): Promise<ProjectRecord> {
    return this.transport.request("project.patch", {
      project_id: projectId,
      name: patch.name ?? null,
    });
  }

  deleteProject(projectId: string): Promise<unknown> {
    return this.transport.request("project.delete", { project_id: projectId });
  }

  readFsDirectory(path: string | null = null): Promise<FsDirectorySnapshot> {
    return this.transport.request("fs.directory.read", { path });
  }

  createFsDirectory(parent: string, name: string): Promise<FsDirectoryCreateResponse> {
    return this.transport.request("fs.directory.create", { parent, name });
  }

  startTurn(params: AppProtocol.TurnStartParams): Promise<TalosMessageAck> {
    return this.transport.request("turn.start", params);
  }

  retryTurn(params: AppProtocol.TurnRetryParams): Promise<TalosMessageAck> {
    return this.transport.request("turn.retry", params);
  }

  interruptTurn(sessionId: string): Promise<unknown> {
    return this.transport.request("turn.interrupt", { session_id: sessionId });
  }

  subscribeEvents(
    params: AppProtocol.EventSubscribeParams,
  ): Promise<AppProtocol.EventSubscribeResponse> {
    return this.transport.request("event.subscribe", params);
  }

  unsubscribeEvents(
    subscriptionId: string,
  ): Promise<AppProtocol.EventUnsubscribeResponse> {
    return this.transport.request("event.unsubscribe", { subscription_id: subscriptionId });
  }

  submitDynamicToolResult(params: AppProtocol.ToolDynamicResultSubmitParams): Promise<unknown> {
    return this.transport.request("tool.dynamic_result.submit", params);
  }

  listMemoryFiles(limit?: number): Promise<MemoryFileSummary[]> {
    return this.transport
      .request<ListResponse<MemoryFileSummary>>("memory.file.list", { limit: limit ?? null })
      .then((response) => response.data);
  }

  readMemoryFile(path: string): Promise<MemoryFileDetail> {
    return this.transport.request("memory.file.read", { path });
  }

  searchMemory(query: string, limit?: number): Promise<MemorySearchResult[]> {
    return this.transport
      .request<ListResponse<MemorySearchResult>>("memory.search", {
        query,
        limit: limit ?? null,
      })
      .then((response) => response.data);
  }

  writeMemoryFile(
    path: string,
    content: string,
    expectedVersion: string | null,
  ): Promise<MemoryFileDetail> {
    return this.transport.request("memory.file.write", {
      path,
      content,
      expected_version: expectedVersion,
    });
  }

  listWorkspaceDocs(): Promise<WorkspaceDocSummary[]> {
    return this.transport
      .request<ListResponse<WorkspaceDocSummary>>("workspace.doc.list", {})
      .then((response) => response.data);
  }

  getWorkspaceInfo(): Promise<AgentWorkspaceInfo> {
    return this.transport.request("workspace.info", {});
  }

  readWorkspaceDoc(kind: WorkspaceDocKind): Promise<WorkspaceDocDetail> {
    return this.transport.request("workspace.doc.read", { kind });
  }

  writeWorkspaceDoc(
    kind: WorkspaceDocKind,
    content: string,
    expectedVersion: string | null,
  ): Promise<WorkspaceDocDetail> {
    return this.transport.request("workspace.doc.write", {
      kind,
      content,
      expected_version: expectedVersion,
    });
  }
}

function normalizeTimelineRecoverySnapshot(
  snapshot: TimelineRecoverySnapshot,
): TimelineRecoverySnapshot {
  return {
    ...snapshot,
    replay: {
      ...snapshot.replay,
      events: snapshot.replay.events.map((event) => ({
        ...event,
        event_seq: Number(event.event_seq),
        created_at_ms: Number(event.created_at_ms),
        event: {
          ...event.event,
          event_seq: event.event.event_seq == null ? event.event.event_seq : Number(event.event.event_seq),
          created_at_ms: event.event.created_at_ms == null ? event.event.created_at_ms : Number(event.event.created_at_ms),
          sequence: Number(event.event.sequence),
        },
      })),
      last_replay_seq: Number(snapshot.replay.last_replay_seq),
    },
    active_turn: {
      ...snapshot.active_turn,
      updated_at: Number(snapshot.active_turn.updated_at),
    },
  };
}

function normalizeSessionHistorySnapshot(
  snapshot: TalosSessionHistorySnapshot,
): TalosSessionHistorySnapshot {
  return {
    ...snapshot,
    generated_at_ms: Number(snapshot.generated_at_ms),
    messages: snapshot.messages.map((message) => ({
      ...message,
      items: message.items.map((item) => ({
        ...item,
        started_at_ms: Number(item.started_at_ms),
        completed_at_ms:
          item.completed_at_ms == null ? item.completed_at_ms : Number(item.completed_at_ms),
      })),
      usage: {
        ...normalizeUsage(message.usage),
      },
      metrics: normalizeTurnExecutionMetrics(message.metrics),
    })),
  };
}

function normalizeTurnExecutionMetrics(
  metrics: ({
    model_rounds: bigint | number;
    tool_calls: bigint | number;
    tool_successes: bigint | number;
    tool_errors: bigint | number;
    stop_reason: TalosTurnExecutionMetrics["stop_reason"];
    usage: TalosUsage;
    duration_ms: bigint | number | null;
  }) | null,
): TalosTurnExecutionMetrics | null {
  if (metrics == null) {
    return null;
  }
  return {
    ...metrics,
    model_rounds: Number(metrics.model_rounds),
    tool_calls: Number(metrics.tool_calls),
    tool_successes: Number(metrics.tool_successes),
    tool_errors: Number(metrics.tool_errors),
    duration_ms: metrics.duration_ms == null ? metrics.duration_ms : Number(metrics.duration_ms),
    usage: {
      ...normalizeUsage(metrics.usage),
    },
  };
}

function normalizeUsage(usage: TalosUsage): TalosUsage {
  return {
    ...usage,
    input_tokens: Number(usage.input_tokens),
    output_tokens: Number(usage.output_tokens),
    total_tokens: Number(usage.total_tokens),
    cache_creation_input_tokens:
      usage.cache_creation_input_tokens == null
        ? usage.cache_creation_input_tokens
        : Number(usage.cache_creation_input_tokens),
    cache_read_input_tokens:
      usage.cache_read_input_tokens == null
        ? usage.cache_read_input_tokens
        : Number(usage.cache_read_input_tokens),
  };
}
