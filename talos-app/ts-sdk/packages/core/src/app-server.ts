import type * as AppProtocol from "./generated/app-protocol";
import type {
  MemoryFileDetail,
  MemoryFileSummary,
  MemorySearchResult,
  LlmConfig,
  LlmConfigRead,
  ModelInfo,
  ProjectPatch,
  ProjectRecord,
  AdminCreateUserRequest,
  AdminPatchUserRequest,
  AuthResponse,
  CreateUserTokenRequest,
  CreateUserTokenResult,
  LoginRequest,
  RegisterRequest,
  TalosActiveTurn,
  TalosMessageAck,
  TalosSessionHistorySnapshot,
  TalosSession,
  TimelineRecoverySnapshot,
  UpdateCurrentUserRequest,
  UserDto,
  UserTokenMeta,
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

export interface AdminSessionListResponse {
  data: TalosSession[];
  pagination: AppProtocol.AdminSessionPagination;
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

  getAccount(): Promise<unknown> {
    return this.transport.request("account.get", {});
  }

  authRegister(request: RegisterRequest): Promise<AuthResponse> {
    return this.transport.request("auth.register", request);
  }

  authLogin(request: LoginRequest): Promise<AuthResponse> {
    return this.transport.request("auth.login", request);
  }

  authLogout(): Promise<unknown> {
    return this.transport.request("auth.logout", {});
  }

  getCurrentUser(): Promise<UserDto> {
    return this.transport.request("user.current", {});
  }

  updateCurrentUser(request: UpdateCurrentUserRequest): Promise<UserDto> {
    return this.transport.request("user.update", request);
  }

  listUserTokens(): Promise<UserTokenMeta[]> {
    return this.transport
      .request<ListResponse<UserTokenMeta>>("user.token.list", {})
      .then((response) => response.data);
  }

  createUserToken(request: CreateUserTokenRequest): Promise<CreateUserTokenResult> {
    return this.transport.request("user.token.create", request);
  }

  deleteUserToken(tokenId: string): Promise<unknown> {
    return this.transport.request("user.token.delete", { token_id: tokenId });
  }

  listSkills(): Promise<unknown[]> {
    return this.transport
      .request<ListResponse<unknown>>("skill.list", {})
      .then((response) => response.data);
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

  createSession(params: AppProtocol.SessionCreateParams): Promise<TalosSession> {
    return this.transport.request("session.create", params);
  }

  listSessions(params: AppProtocol.SessionListParams): Promise<TalosSession[]> {
    return this.transport
      .request<ListResponse<TalosSession>>("session.list", params)
      .then((response) => response.data);
  }

  listAdminSessions(
    params: AppProtocol.AdminSessionListParams,
  ): Promise<AdminSessionListResponse> {
    return this.transport.request("admin.session.list", params);
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

  listAdminProjects(): Promise<ProjectRecord[]> {
    return this.transport
      .request<ListResponse<ProjectRecord>>("admin.project.list", {})
      .then((response) => response.data);
  }

  listAdminUsers(): Promise<UserDto[]> {
    return this.transport
      .request<ListResponse<UserDto>>("admin.user.list", {})
      .then((response) => response.data);
  }

  createAdminUser(request: AdminCreateUserRequest): Promise<UserDto> {
    return this.transport.request("admin.user.create", request);
  }

  patchAdminUser(userId: string, request: AdminPatchUserRequest): Promise<UserDto> {
    return this.transport.request("admin.user.patch", {
      user_id: userId,
      ...request,
    });
  }

  deleteAdminUser(userId: string): Promise<unknown> {
    return this.transport.request("admin.user.delete", { user_id: userId });
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
        ...message.usage,
        input_tokens: Number(message.usage.input_tokens),
        output_tokens: Number(message.usage.output_tokens),
        total_tokens: Number(message.usage.total_tokens),
      },
    })),
  };
}
