import { decodeFileContent } from "./file-content";
import type { FileUploadOptions, HttpAdapter, StreamAdapter } from "./adapters";
import { parseTalosJson, parseTimelineEvent } from "./adapters";
import type {
  DocUpdateRequest,
  ConfigPath,
  ConfigResponse,
  ConfigValue,
  LlmConfig,
  MemoryFileListRequest,
  MemoryFileListResponse,
  MemoryFileReadRequest,
  MemoryFileReadResponse,
  MemoryFileWriteRequest,
  MemoryFileWriteResponse,
  MemoryGetRequest,
  MemoryGetResponse,
  MemorySearchRequest,
  MemorySearchResponse,
  MessageQueue,
  MessageQueueAddRequest,
  MessageQueueChangedNotification,
  MessageQueueDeleteRequest,
  MessageQueueListRequest,
  MessageQueueReorderRequest,
  MessageQueueStartRequest,
  MessageQueueUpdateRequest,
  ModelListResponse,
  ProfileReadResponse,
  PluginDisableRequest,
  PluginDisableResponse,
  PluginEnableRequest,
  PluginEnableResponse,
  PluginInstallRequest,
  PluginInstallResponse,
  McpAuthenticationStartRequest, McpAuthenticationStartResponse,
  McpServerImportRequest, McpAuthenticationReadRequest, McpCredentialsUpdateRequest, McpAuthenticationResponse,
  McpServerListRequest,
  McpServerListResponse,
  PluginListRequest,
  PluginListResponse,
  PluginReadRequest,
  PluginReadResponse,
  PluginSearchRequest,
  PluginSearchResponse,
  PluginSourceAddRequest,
  PluginSourceAddResponse,
  PluginSourceListRequest,
  PluginSourceListResponse,
  PluginSourceRefreshRequest,
  PluginSourceRefreshResponse,
  PluginSourceRemoveRequest,
  PluginSourceRemoveResponse,
  PluginUpdateRequest,
  PluginUpdateResponse,
  PluginUninstallRequest,
  PluginUninstallResponse,
  UserCurrentResponse,
  FsDirectoryCreateRequest,
  FsDirectoryCreateResponse,
  FsDirectoryReadRequest,
  FsDirectoryReadResponse,
  FileDeleteRequest,
  FileDeleteResponse,
  FileListRequest,
  FileListResponse,
  FileReadRequest,
  FileReadResponse,
  FileUpdateRequest,
  FileUpdateResponse,
  FileUploadData,
  FileUploadRequest,
  FileUploadResponse,
  ProjectCreateRequest,
  ProjectCreateResponse,
  ProjectDeleteRequest,
  ProjectDeleteResponse,
  ProjectListResponse,
  ProjectListRequest,
  ProjectPatch,
  ProjectPatchRequest,
  ProjectPatchResponse,
  ProjectReadRequest,
  ProjectReadResponse,
  SessionActiveTurnRequest,
  SkillsListRequest,
  SkillsListResponse,
  SkillsReadRequest,
  SkillsReadResponse,
  SkillsRefreshRequest,
  SkillsResolveRequest,
  SkillsResolveResponse,
  SkillsSuggestRequest,
  SkillsSuggestResponse,
  SessionPatchRequest,
  SessionClearRequest,
  SessionCompactResponse,
  SessionCompactRequest,
  SessionClearResponse,
  SessionCloseRequest,
  SessionCloseResponse,
  SessionDeleteRequest,
  SessionDeleteResponse,
  SessionGetRequest,
  SessionGetResponse,
  SessionActiveTurnResponse,
  SessionHistoryRequest,
  SessionHistoryResponse,
  SessionListRequest,
  SessionListResponse,
  SessionUpdateResponse,
  SubmitTurnInputRequest,
  SubmitTurnInputResponse,
  InterruptTurnResponse,
  InterruptTurnRequest,
  RetryTurnRequest,
  RecoverTimelineRequest,
  SubscriptionTerminatedEvent,
  StartTurnRequest,
  StartTurnResponse,
  TimelineEvent,
  RecoverTimelineResponse,
  WorkspaceDocKind,
  WorkspaceInfoResponse,
  WorkspaceDocListResponse,
  WorkspaceDocReadResponse,
  WorkspaceDocWriteResponse,
} from "./types";

interface RequestControlOptions {
  signal?: AbortSignal;
}

export interface SessionEventStreamOptions {
  persistent?: boolean;
  onOpen?: () => void;
  onMessageQueueChanged?: (notification: MessageQueueChangedNotification) => void;
}

export class TalosStreamTerminatedError extends Error {
  public readonly reason: SubscriptionTerminatedEvent["reason"];
  public readonly responseMessage: string;

  constructor(event: SubscriptionTerminatedEvent) {
    super(event.message);
    this.name = "TalosStreamTerminatedError";
    this.reason = event.reason;
    this.responseMessage = event.message;
  }
}

export class TalosClient {
  constructor(
    private readonly http: HttpAdapter,
    private readonly streamAdapter: StreamAdapter,
    private readonly baseUrl: string,
  ) {}

  private url(path: string): string {
    return `${this.baseUrl.replace(/\/$/, "")}${path}`;
  }

  private urlWithQuery<T extends object>(path: string, query?: T) {
    if (!query) {
      return this.url(path);
    }
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(query)) {
      if (value !== undefined && value !== null) {
        params.set(key, String(value));
      }
    }
    const serialized = params.toString();
    return serialized ? `${this.url(path)}?${serialized}` : this.url(path);
  }

  auth = {
    logout: async (): Promise<void> => {
      await this.http.request<void>(this.url("/v1/auth/logout"), {
        method: "POST",
      });
    },
  };

  user = {
    current: async (): Promise<UserCurrentResponse> => {
      return this.http.request<UserCurrentResponse>(this.url("/v1/user"));
    },
  };

  profile = {
    read: (): Promise<ProfileReadResponse> =>
      this.http.request<ProfileReadResponse>(this.url("/v1/profile")),
  };

  llm = {
    get: (): Promise<ConfigResponse> => {
      return this.http.request<ConfigResponse>(this.url("/v1/llm"));
    },
    update: (config: LlmConfig): Promise<ConfigResponse> => {
      return this.http.request<ConfigResponse>(this.url("/v1/llm"), {
        method: "PUT",
        body: JSON.stringify(config),
      });
    },
  };

  config = {
    get: (path: ConfigPath = "llm"): Promise<ConfigResponse> => {
      return this.http.request<ConfigResponse>(this.url(`/v1/config/${path}`));
    },
    update: (value: ConfigValue): Promise<ConfigResponse> => {
      return this.http.request<ConfigResponse>(this.url("/v1/config"), {
        method: "PUT",
        body: JSON.stringify(value),
      });
    },
  };

  models = {
    list: (): Promise<ModelListResponse> =>
      this.http.request<ModelListResponse>(this.url("/v1/models")),
  };

  skills = {
    list: (query?: SkillsListRequest): Promise<SkillsListResponse> =>
      this.http.request<SkillsListResponse>(this.urlWithQuery("/v1/skills", query)),
    refresh: (request: SkillsRefreshRequest = {}): Promise<SkillsListResponse> => {
      void request;
      return this.http.request<SkillsListResponse>(this.url("/v1/skills/refresh"), {
        method: "POST",
      });
    },
    suggest: (query: SkillsSuggestRequest): Promise<SkillsSuggestResponse> =>
      this.http.request<SkillsSuggestResponse>(this.urlWithQuery("/v1/skills/suggest", query)),
    read: (request: SkillsReadRequest): Promise<SkillsReadResponse> =>
      this.http.request<SkillsReadResponse>(this.url("/v1/skills/read"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    resolve: (request: SkillsResolveRequest): Promise<SkillsResolveResponse> =>
      this.http.request<SkillsResolveResponse>(this.url("/v1/skills/resolve"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
  };

  mcp = {
    import: (request: McpServerImportRequest): Promise<PluginInstallResponse> => this.http.request<PluginInstallResponse>(this.url("/v1/mcp/servers/import"), { method: "POST", body: JSON.stringify(request) }),
    startAuthentication: (request: McpAuthenticationStartRequest): Promise<McpAuthenticationStartResponse> => this.http.request<McpAuthenticationStartResponse>(this.url("/v1/mcp/servers/auth/start"), { method: "POST", body: JSON.stringify(request) }),
    readAuthentication: (request: McpAuthenticationReadRequest): Promise<McpAuthenticationResponse> => this.http.request<McpAuthenticationResponse>(this.url("/v1/mcp/servers/auth/read"), { method: "POST", body: JSON.stringify(request) }),
    updateCredentials: (request: McpCredentialsUpdateRequest): Promise<McpAuthenticationResponse> => this.http.request<McpAuthenticationResponse>(this.url("/v1/mcp/servers/credentials"), { method: "PUT", body: JSON.stringify(request) }),

    list: (request: McpServerListRequest = {}): Promise<McpServerListResponse> =>
      this.http.request<McpServerListResponse>(this.urlWithQuery("/v1/mcp/servers", request)),
  };

  plugin = {
    search: (request: PluginSearchRequest): Promise<PluginSearchResponse> =>
      this.http.request<PluginSearchResponse>(this.url("/v1/plugin/search"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    list: (request: PluginListRequest): Promise<PluginListResponse> =>
      this.http.request<PluginListResponse>(this.url("/v1/plugin/list"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    read: (request: PluginReadRequest): Promise<PluginReadResponse> =>
      this.http.request<PluginReadResponse>(this.url("/v1/plugin/read"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    install: (request: PluginInstallRequest): Promise<PluginInstallResponse> =>
      this.http.request<PluginInstallResponse>(this.url("/v1/plugin/install"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    update: (request: PluginUpdateRequest): Promise<PluginUpdateResponse> =>
      this.http.request<PluginUpdateResponse>(this.url("/v1/plugin/update"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    uninstall: (request: PluginUninstallRequest): Promise<PluginUninstallResponse> =>
      this.http.request<PluginUninstallResponse>(this.url("/v1/plugin/uninstall"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    enable: (request: PluginEnableRequest): Promise<PluginEnableResponse> =>
      this.http.request<PluginEnableResponse>(this.url("/v1/plugin/enable"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    disable: (request: PluginDisableRequest): Promise<PluginDisableResponse> =>
      this.http.request<PluginDisableResponse>(this.url("/v1/plugin/disable"), {
        method: "POST",
        body: JSON.stringify(request),
    }),
    sources: {
      list: (query: PluginSourceListRequest = {}): Promise<PluginSourceListResponse> =>
        this.http.request<PluginSourceListResponse>(this.urlWithQuery("/v1/plugin/sources", query)),
      add: (request: PluginSourceAddRequest): Promise<PluginSourceAddResponse> =>
        this.http.request<PluginSourceAddResponse>(this.url("/v1/plugin/sources"), {
          method: "POST",
          body: JSON.stringify(request),
        }),
      refresh: (request: PluginSourceRefreshRequest): Promise<PluginSourceRefreshResponse> =>
        this.http.request<PluginSourceRefreshResponse>(this.url("/v1/plugin/sources/refresh"), {
          method: "POST",
          body: JSON.stringify(request),
        }),
      remove: (request: PluginSourceRemoveRequest): Promise<PluginSourceRemoveResponse> =>
        this.http.request<PluginSourceRemoveResponse>(this.url("/v1/plugin/sources/remove"), {
          method: "POST",
          body: JSON.stringify(request),
        }),
    },
  };

  sessions = {
    list: (query?: SessionListRequest): Promise<SessionListResponse> =>
      this.http.request<SessionListResponse>(this.urlWithQuery("/v1/sessions", query)),
    get: (request: SessionGetRequest): Promise<SessionGetResponse> =>
      this.http.request<SessionGetResponse>(this.url(`/v1/sessions/${request.sessionId}`)),
    getById: (sessionId: string): Promise<SessionGetResponse> =>
      this.sessions.get({ sessionId }),
    patch: (request: SessionPatchRequest): Promise<SessionUpdateResponse> =>
      this.http.request<SessionUpdateResponse>(this.url(`/v1/sessions/${request.sessionId}`), {
        method: "PATCH",
        body: JSON.stringify(request),
      }),
    close: async (request: SessionCloseRequest): Promise<SessionCloseResponse> => {
      const response = await this.http.request<SessionCloseResponse>(this.url(`/v1/sessions/${request.sessionId}/close`), {
        method: "POST",
      });
      if (!response.success) {
        throw new Error("Session close did not succeed.");
      }
      return response;
    },
    closeById: (sessionId: string): Promise<SessionCloseResponse> =>
      this.sessions.close({ sessionId }),
    delete: async (request: SessionDeleteRequest): Promise<SessionDeleteResponse> => {
      const response = await this.http.request<SessionDeleteResponse>(this.url(`/v1/sessions/${request.sessionId}`), {
        method: "DELETE",
      });
      if (!response.success) {
        throw new Error("Session delete did not succeed.");
      }
      return response;
    },
    deleteById: (sessionId: string): Promise<SessionDeleteResponse> =>
      this.sessions.delete({ sessionId }),
    cancel: async (request: InterruptTurnRequest): Promise<InterruptTurnResponse> => {
      const response = await this.http.request<InterruptTurnResponse>(
        this.url("/v1/turns/interrupt"),
        {
          method: "POST",
          body: JSON.stringify(request),
        },
      );
      return response;
    },
    cancelById: (sessionId: string, turnId: string): Promise<InterruptTurnResponse> =>
      this.sessions.cancel({
        identity: { sessionId, turnId },
      }),
    compact: (request: SessionCompactRequest) =>
      this.http.request<SessionCompactResponse>(this.url(`/v1/sessions/${request.sessionId}/compact`), {
        method: "POST",
      }),
    compactById: (sessionId: string): Promise<SessionCompactResponse> =>
      this.sessions.compact({ sessionId }),
    clear: (request: SessionClearRequest) =>
      this.http.request<SessionClearResponse>(this.url(`/v1/sessions/${request.sessionId}/clear`), {
        method: "POST",
      }),
    clearById: (sessionId: string): Promise<SessionClearResponse> =>
      this.sessions.clear({ sessionId }),
    activeTurn: (request: SessionActiveTurnRequest) =>
      this.http.request<SessionActiveTurnResponse>(this.url(`/v1/sessions/${request.sessionId}/active-turn`)),
    activeTurnById: (sessionId: string): Promise<SessionActiveTurnResponse> =>
      this.sessions.activeTurn({ sessionId }),
    history: async (request: SessionHistoryRequest): Promise<SessionHistoryResponse> => {
      return this.http.request<SessionHistoryResponse>(
        this.urlWithQuery(`/v1/sessions/${request.sessionId}/history`, {
          cursor: request.cursor ?? undefined,
          limit: request.limit ?? undefined,
        }),
      );
    },
    historyById: (sessionId: string): Promise<SessionHistoryResponse> =>
      this.sessions.history({ sessionId, cursor: null, limit: null }),
    recoverTimeline: async (request: RecoverTimelineRequest): Promise<RecoverTimelineResponse> => {
      return this.http.request<RecoverTimelineResponse>(
        this.urlWithQuery(`/v1/sessions/${request.sessionId}/timeline/recovery`, {
          after_event_id: request.afterEventId ?? undefined,
          limit: request.replayLimit ?? undefined,
        }),
      );
    },
    recoverTimelineById: (
      sessionId: string,
      afterEventId?: string,
      replayLimit?: number,
    ): Promise<RecoverTimelineResponse> =>
      this.sessions.recoverTimeline({
        sessionId,
        afterEventId: afterEventId ?? null,
        replayLimit: replayLimit ?? null,
      }),
  };

  projects = {
    list: (request: ProjectListRequest = {}): Promise<ProjectListResponse> => {
      void request;
      return this.http.request<ProjectListResponse>(this.url("/v1/projects"));
    },
    get: (request: ProjectReadRequest): Promise<ProjectReadResponse> =>
      this.http.request<ProjectReadResponse>(this.url(`/v1/projects/${request.project_id}`)),
    getById: (projectId: string): Promise<ProjectReadResponse> =>
      this.projects.get({ project_id: projectId }),
    create: (body: ProjectCreateRequest): Promise<ProjectCreateResponse> =>
      this.http.request<ProjectCreateResponse>(this.url("/v1/projects"), {
        method: "POST",
        body: JSON.stringify(body),
      }),
    patch: (request: ProjectPatchRequest): Promise<ProjectPatchResponse> =>
      this.http.request<ProjectPatchResponse>(this.url(`/v1/projects/${request.project_id}`), {
        method: "PATCH",
        body: JSON.stringify(request),
      }),
    patchById: (projectId: string, patch: ProjectPatch): Promise<ProjectPatchResponse> =>
      this.projects.patch({ project_id: projectId, name: patch.name }),
    delete: (request: ProjectDeleteRequest): Promise<ProjectDeleteResponse> =>
      this.http.request<ProjectDeleteResponse>(this.url(`/v1/projects/${request.project_id}`), {
        method: "DELETE",
      }),
    deleteById: (projectId: string): Promise<ProjectDeleteResponse> =>
      this.projects.delete({ project_id: projectId }),
  };

  files = {
    upload: (
      request: FileUploadRequest,
      data: FileUploadData,
      options: FileUploadOptions = {},
    ): Promise<FileUploadResponse> => {
      const form = new FormData();
      form.append(
        "metadata",
        new Blob([JSON.stringify(request)], { type: "application/json" }),
      );
      let file: Blob;
      if (data instanceof Blob) {
        file = data;
      } else {
        const bytes = new Uint8Array(data.byteLength);
        bytes.set(data);
        file = new Blob([bytes.buffer]);
      }
      form.append("file", file, request.filename);
      return this.http.request<FileUploadResponse>(this.url("/v1/files"), {
        method: "POST",
        body: form,
        signal: options.signal,
        onUploadProgress: options.onProgress,
      });
    },
    list: (request: FileListRequest = {}): Promise<FileListResponse> => {
      const query = new URLSearchParams();
      if (request.cursor !== undefined) {
        query.set("cursor", request.cursor);
      }
      if (request.limit !== undefined) {
        query.set("limit", String(request.limit));
      }
      if (request.scope?.type === "personal") {
        query.set("scope", "personal");
      } else if (request.scope?.type === "project") {
        query.set("scope", "project");
        query.set("projectId", request.scope.projectId);
      }
      for (const tag of request.tags ?? []) {
        query.append("tag", tag);
      }
      const serialized = query.toString();
      return this.http.request<FileListResponse>(
        this.url(`/v1/files${serialized ? `?${serialized}` : ""}`),
      );
    },
    read: (request: FileReadRequest): Promise<FileReadResponse> =>
      this.http.request<FileReadResponse>(
        this.url(`/v1/files/${encodeURIComponent(request.fileId)}`),
      ),
    content: (request: FileReadRequest): Promise<import("./types").FileDownloadResponse> =>
      this.http.request<import("./generated/app-protocol").FileContentResponse>(this.url(`/v1/files/${encodeURIComponent(request.fileId)}/content`)).then(decodeFileContent),
    update: (request: FileUpdateRequest): Promise<FileUpdateResponse> => {
      const { fileId, ...body } = request;
      return this.http.request<FileUpdateResponse>(
        this.url(`/v1/files/${encodeURIComponent(fileId)}`),
        {
          method: "PATCH",
          body: JSON.stringify(body),
        },
      );
    },
    delete: (request: FileDeleteRequest): Promise<FileDeleteResponse> =>
      this.http.request<FileDeleteResponse>(
        this.url(`/v1/files/${encodeURIComponent(request.fileId)}`),
        { method: "DELETE" },
      ),
  };

  fs = {
    directory: {
      read: (query: FsDirectoryReadRequest = { path: null }): Promise<FsDirectoryReadResponse> =>
        this.http.request<FsDirectoryReadResponse>(
          this.urlWithQuery("/v1/fs/directory", {
            path: query?.path ?? undefined,
          }),
        ),
      create: (body: FsDirectoryCreateRequest): Promise<FsDirectoryCreateResponse> =>
        this.http.request<FsDirectoryCreateResponse>(this.url("/v1/fs/directory"), {
          method: "POST",
          body: JSON.stringify(body),
        }),
    },
  };

  memory = {
    list: (query?: MemoryFileListRequest): Promise<MemoryFileListResponse> =>
      this.http.request<MemoryFileListResponse>(
        this.urlWithQuery("/v1/memory/files", query),
      ),
    get: (request: MemoryFileReadRequest) =>
      this.http.request<MemoryFileReadResponse>(this.url(`/v1/memory/files/${request.date}`)),
    put: (request: MemoryFileWriteRequest) =>
      this.http.request<MemoryFileWriteResponse>(this.url(`/v1/memory/files/${request.date}`), {
        method: "PUT",
        body: JSON.stringify(request),
      }),
    search: async (
      query: MemorySearchRequest,
      options?: RequestControlOptions,
    ): Promise<MemorySearchResponse> => {
      const requestOptions = options?.signal ? { signal: options.signal } : undefined;
      const url = this.urlWithQuery("/v1/memory/search", query);
      return requestOptions
        ? await this.http.request<MemorySearchResponse>(url, requestOptions)
        : await this.http.request<MemorySearchResponse>(url);
    },
    getEntry: (request: MemoryGetRequest): Promise<MemoryGetResponse> =>
      this.http.request<MemoryGetResponse>(
        this.url(`/v1/memory/entries/${encodeURIComponent(request.memoryId)}`),
      ),
  };

  workspaceDocs = {
    info: (): Promise<WorkspaceInfoResponse> =>
      this.http.request<WorkspaceInfoResponse>(this.url("/v1/workspace/info")),
    list: (): Promise<WorkspaceDocListResponse> =>
      this.http.request<WorkspaceDocListResponse>(this.url("/v1/workspace/docs")),
    get: (kind: WorkspaceDocKind) =>
      this.http.request<WorkspaceDocReadResponse>(this.url(`/v1/workspace/docs/${kind}`)),
    put: (kind: WorkspaceDocKind, request: DocUpdateRequest) =>
      this.http.request<WorkspaceDocWriteResponse>(this.url(`/v1/workspace/docs/${kind}`), {
        method: "PUT",
        body: JSON.stringify(request),
      }),
  };

  messageQueue = {
    add: (request: MessageQueueAddRequest): Promise<MessageQueue> =>
      this.http.request<MessageQueue>(this.url("/v1/message-queue/add"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    list: (request: MessageQueueListRequest): Promise<MessageQueue> =>
      this.http.request<MessageQueue>(this.url("/v1/message-queue/list"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    update: (request: MessageQueueUpdateRequest): Promise<MessageQueue> =>
      this.http.request<MessageQueue>(this.url("/v1/message-queue/update"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    delete: (request: MessageQueueDeleteRequest): Promise<MessageQueue> =>
      this.http.request<MessageQueue>(this.url("/v1/message-queue/delete"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    reorder: (request: MessageQueueReorderRequest): Promise<MessageQueue> =>
      this.http.request<MessageQueue>(this.url("/v1/message-queue/reorder"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    start: (request: MessageQueueStartRequest): Promise<StartTurnResponse> =>
      this.http.request<StartTurnResponse>(this.url("/v1/message-queue/start"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
  };

  turns = {
    input: (request: SubmitTurnInputRequest): Promise<SubmitTurnInputResponse> =>
      this.http.request<SubmitTurnInputResponse>(this.url("/v1/turns/input"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    start: (request: StartTurnRequest) =>
      this.turns.startWithIdempotencyKey(request, request.clientRequestId ?? createIdempotencyKey()),
    startWithIdempotencyKey: (
      request: StartTurnRequest,
      idempotencyKey: string,
    ) =>
      this.http.request<StartTurnResponse>(this.url("/v1/turns/start"), {
        method: "POST",
        headers: {
          "idempotency-key": idempotencyKey,
        },
        body: JSON.stringify({
          ...request,
          clientRequestId: idempotencyKey,
        }),
      }),
    retryWithIdempotencyKey: (
      request: RetryTurnRequest,
      idempotencyKey: string,
    ) =>
      this.http.request<StartTurnResponse>(this.url("/v1/turns/retry"), {
        method: "POST",
        headers: {
          "idempotency-key": idempotencyKey,
        },
        body: JSON.stringify(request),
      }),
    retry: (request: RetryTurnRequest) =>
      this.turns.retryWithIdempotencyKey(
        request,
        `retry_${request.clientMessageId}_${request.retryOfMessageId}`,
      ),
    events: (
      sessionId: string,
      afterEventId: string | undefined,
      onEvent: (event: TimelineEvent) => void,
      signal?: AbortSignal,
      options?: SessionEventStreamOptions,
    ) => {
      const query = new URLSearchParams();
      if (afterEventId) {
        query.set("after_event_id", afterEventId);
      }
      if (options?.persistent) {
        query.set("close_when_idle", "false");
      }
      const serializedQuery = query.toString();
      return this.streamAdapter.stream(
        this.url(`/v1/sessions/${sessionId}/events${serializedQuery ? `?${serializedQuery}` : ""}`),
        {
          method: "GET",
          headers: {
            Accept: "text/event-stream",
          },
          signal,
          onOpen: options?.onOpen,
        },
        (data, event) => {
          if (data === "[DONE]") {
            return;
          }
          const payload = parseTalosJson(data);
          if (event === "messageQueue/changed") {
            options?.onMessageQueueChanged?.(payload as MessageQueueChangedNotification);
            return;
          }
          const terminated = normalizeSubscriptionTerminatedEvent(payload);
          if (terminated) {
            if (terminated.reason === "completed") {
              return;
            }
            throw new TalosStreamTerminatedError(terminated);
          }
          onEvent(parseTimelineEvent(payload));
        },
      );
    },
    eventsForTurn: async (
      sessionId: string,
      turnId: string,
      onEvent: (event: TimelineEvent) => void,
      signal?: AbortSignal,
      afterEventId?: string,
    ): Promise<void> => {
      const controller = new AbortController();
      let completed = false;
      const abortFromCaller = () => controller.abort();
      if (signal?.aborted) {
        controller.abort();
      } else {
        signal?.addEventListener("abort", abortFromCaller, { once: true });
      }

      try {
        await this.turns.events(
          sessionId,
          afterEventId,
          (record) => {
            if (record.event.data.identity.turnId !== turnId) {
              return;
            }
            onEvent(record);
            if (isTerminalTimelineEvent(record)) {
              completed = true;
              controller.abort();
            }
          },
          controller.signal,
        );
      } catch (error) {
        if (!completed) {
          throw error;
        }
      } finally {
        signal?.removeEventListener("abort", abortFromCaller);
      }
    },
    startEvents: async (
      request: StartTurnRequest,
      onEvent: (event: TimelineEvent) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const ack = await this.turns.start({
        ...request,
        clientRequestId: request.clientRequestId ?? createIdempotencyKey(),
      });
      await this.turns.eventsForTurn(request.sessionId, ack.turnId, onEvent, signal);
    },
    retryEvents: async (
      request: RetryTurnRequest,
      onEvent: (event: TimelineEvent) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const ack = await this.turns.retry(request);
      await this.turns.eventsForTurn(request.sessionId, ack.turnId, onEvent, signal);
    },
  };

}

function createIdempotencyKey() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return `msg_${globalThis.crypto.randomUUID()}`;
  }
  return `msg_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}

function normalizeSubscriptionTerminatedEvent(
  value: unknown,
): SubscriptionTerminatedEvent | null {
  if (!value || typeof value !== "object") {
    return null;
  }
  const candidate = value as Partial<SubscriptionTerminatedEvent>;
  if (typeof candidate.reason !== "string" || typeof candidate.message !== "string") {
    return null;
  }
  if (
    candidate.reason !== "completed" &&
    candidate.reason !== "interrupted" &&
    candidate.reason !== "server_shutdown" &&
    candidate.reason !== "session_deleted" &&
    candidate.reason !== "lagged" &&
    candidate.reason !== "slow_consumer"
  ) {
    return null;
  }
  return {
    reason: candidate.reason,
    message: candidate.message,
  };
}

function isTerminalTimelineEvent(event: TimelineEvent): boolean {
  return event.event.type === "agent" && (event.event.data.payload.type === "turnCompleted" || event.event.data.payload.type === "turnFailed");
}
