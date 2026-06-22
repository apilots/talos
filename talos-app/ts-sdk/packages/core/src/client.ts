import type { HttpAdapter, StreamAdapter } from "./adapters";
import type {
  DocUpdateRequest,
  ConfigGetResponse,
  ConfigUpdateResponse,
  LlmConfig,
  MemoryFileListRequest,
  MemoryFileListResponse,
  MemoryFileReadResponse,
  MemoryFileWriteResponse,
  MemorySearchQuery,
  MemorySearchResponse,
  ModelListResponse,
  PluginDisableRequest,
  PluginDisableResponse,
  PluginEnableRequest,
  PluginEnableResponse,
  PluginInstallRequest,
  PluginInstallResponse,
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
  FsDirectoryCreateResponse,
  FsDirectoryReadResponse,
  ProjectCreateRequest,
  ProjectCreateResponse,
  ProjectDeleteResponse,
  ProjectListResponse,
  ProjectPatch,
  ProjectPatchResponse,
  ProjectReadResponse,
  SkillsListRequest,
  SkillsListResponse,
  SkillsReadRequest,
  SkillsReadResponse,
  SkillsRefreshRequest,
  SkillsResolveRequest,
  SkillsResolveResponse,
  SkillsSuggestRequest,
  SkillsSuggestResponse,
  SessionUpdate,
  SessionCompactResponse,
  SessionClearResponse,
  SessionCloseResponse,
  SessionDeleteResponse,
  SessionGetResponse,
  SessionActiveTurnResponse,
  SessionHistoryResponse,
  SessionListRequest,
  SessionListResponse,
  SessionUpdateResponse,
  InterruptTurnResponse,
  RetryTurnRequest,
  StartTurnRequest,
  StartTurnResponse,
  TimelineEventEnvelope,
  RetryTurnResponse,
  TimelineRecoveryResponse,
  WorkspaceDocKind,
  WorkspaceDocListResponse,
  WorkspaceDocReadResponse,
  WorkspaceDocWriteResponse,
  AgentWorkspaceInfo,
} from "./types";

interface RequestControlOptions {
  signal?: AbortSignal;
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

  llm = {
    get: (): Promise<ConfigGetResponse> => {
      return this.http.request<ConfigGetResponse>(this.url("/v1/llm"));
    },
    update: (config: LlmConfig): Promise<ConfigUpdateResponse> => {
      return this.http.request<ConfigUpdateResponse>(this.url("/v1/llm"), {
        method: "PUT",
        body: JSON.stringify(config),
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
    refresh: (_request: SkillsRefreshRequest = {}): Promise<SkillsListResponse> =>
      this.http.request<SkillsListResponse>(this.url("/v1/skills/refresh"), {
        method: "POST",
      }),
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
    get: (sessionId: string) =>
      this.http.request<SessionGetResponse>(this.url(`/v1/sessions/${sessionId}`)),
    patch: (sessionId: string, update: SessionUpdate): Promise<SessionUpdateResponse> =>
      this.http.request<SessionUpdateResponse>(this.url(`/v1/sessions/${sessionId}`), {
        method: "PATCH",
        body: JSON.stringify(update),
      }),
    close: async (sessionId: string): Promise<SessionCloseResponse> => {
      const response = await this.http.request<SessionCloseResponse>(this.url(`/v1/sessions/${sessionId}/close`), {
        method: "POST",
      });
      if (!response.success) {
        throw new Error("Session close did not succeed.");
      }
      return response;
    },
    delete: async (sessionId: string): Promise<SessionDeleteResponse> => {
      const response = await this.http.request<SessionDeleteResponse>(this.url(`/v1/sessions/${sessionId}`), {
        method: "DELETE",
      });
      if (!response.success) {
        throw new Error("Session delete did not succeed.");
      }
      return response;
    },
    cancel: async (sessionId: string): Promise<InterruptTurnResponse> => {
      const response = await this.http.request<InterruptTurnResponse>(
        this.url("/v1/turns/interrupt"),
        {
          method: "POST",
          body: JSON.stringify({ session_id: sessionId }),
        },
      );
      if (!response.success) {
        throw new Error("Session cancel did not succeed.");
      }
      return response;
    },
    compact: (sessionId: string) =>
      this.http.request<SessionCompactResponse>(this.url(`/v1/sessions/${sessionId}/compact`), {
        method: "POST",
      }),
    clear: (sessionId: string) =>
      this.http.request<SessionClearResponse>(this.url(`/v1/sessions/${sessionId}/clear`), {
        method: "POST",
      }),
    activeTurn: (sessionId: string) =>
      this.http.request<SessionActiveTurnResponse>(this.url(`/v1/sessions/${sessionId}/active-turn`)),
    history: async (sessionId: string): Promise<SessionHistoryResponse> => {
      return this.http.request<SessionHistoryResponse>(
        this.url(`/v1/sessions/${sessionId}/history`),
      );
    },
    recover: async (
      sessionId: string,
      afterEventId?: string,
      replayLimit?: number,
    ): Promise<TimelineRecoveryResponse> => {
      return this.http.request<TimelineRecoveryResponse>(
        this.urlWithQuery(`/v1/sessions/${sessionId}/recovery`, {
          after_event_id: afterEventId,
          limit: replayLimit,
        }),
      );
    },
  };

  projects = {
    list: (): Promise<ProjectListResponse> =>
      this.http.request<ProjectListResponse>(this.url("/v1/projects")),
    get: (projectId: string): Promise<ProjectReadResponse> =>
      this.http.request<ProjectReadResponse>(this.url(`/v1/projects/${projectId}`)),
    create: (body: ProjectCreateRequest): Promise<ProjectCreateResponse> =>
      this.http.request<ProjectCreateResponse>(this.url("/v1/projects"), {
        method: "POST",
        body: JSON.stringify(body),
      }),
    patch: (projectId: string, patch: ProjectPatch): Promise<ProjectPatchResponse> =>
      this.http.request<ProjectPatchResponse>(this.url(`/v1/projects/${projectId}`), {
        method: "PATCH",
        body: JSON.stringify(patch),
      }),
    delete: (projectId: string): Promise<ProjectDeleteResponse> =>
      this.http.request<ProjectDeleteResponse>(this.url(`/v1/projects/${projectId}`), {
        method: "DELETE",
      }),
  };

  fs = {
    directory: {
      read: (query?: { path?: string | null }): Promise<FsDirectoryReadResponse> =>
        this.http.request<FsDirectoryReadResponse>(
          this.urlWithQuery("/v1/fs/directory", {
            path: query?.path ?? undefined,
          }),
        ),
      create: (body: { parent: string; name: string }): Promise<FsDirectoryCreateResponse> =>
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
    get: (date: string) =>
      this.http.request<MemoryFileReadResponse>(this.url(`/v1/memory/files/${date}`)),
    put: (date: string, request: DocUpdateRequest) =>
      this.http.request<MemoryFileWriteResponse>(this.url(`/v1/memory/files/${date}`), {
        method: "PUT",
        body: JSON.stringify(request),
      }),
    search: async (
      query: MemorySearchQuery,
      options?: RequestControlOptions,
    ): Promise<MemorySearchResponse> => {
      const requestOptions = options?.signal ? { signal: options.signal } : undefined;
      const url = this.urlWithQuery("/v1/memory/search", query);
      return requestOptions
        ? await this.http.request<MemorySearchResponse>(url, requestOptions)
        : await this.http.request<MemorySearchResponse>(url);
    },
  };

  workspaceDocs = {
    info: () =>
      this.http.request<AgentWorkspaceInfo>(this.url("/v1/workspace/info")),
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

  turns = {
    start: (request: StartTurnRequest) =>
      this.turns.startWithIdempotencyKey(request, request.client_request_id ?? createIdempotencyKey()),
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
          client_request_id: idempotencyKey,
        }),
      }),
    retryWithIdempotencyKey: (
      request: RetryTurnRequest,
      _idempotencyKey: string,
    ) =>
      this.http.request<RetryTurnResponse>(this.url("/v1/turns/retry"), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    retry: (request: RetryTurnRequest) =>
      this.turns.retryWithIdempotencyKey(
        request,
        `retry_${request.client_message_id}_${request.retry_of_message_id}`,
      ),
    events: (
      sessionId: string,
      afterEventId: string | undefined,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ) => {
      const query = afterEventId
        ? `?after_event_id=${encodeURIComponent(afterEventId)}`
        : "";
      return this.streamAdapter.stream(
        this.url(`/v1/sessions/${sessionId}/events${query}`),
        {
          method: "GET",
          headers: {
            Accept: "text/event-stream",
          },
          signal,
        },
        (data) => {
          if (data === "[DONE]") {
            return;
          }
          onEvent(normalizeTimelineEnvelope(JSON.parse(data)));
        },
      );
    },
    eventsForMessage: async (
      sessionId: string,
      messageId: string,
      onEvent: (event: TimelineEventEnvelope) => void,
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
            if (record.message_id !== messageId) {
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
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const ack = await this.turns.start({
        ...request,
        client_request_id: request.client_request_id ?? createIdempotencyKey(),
      });
      if (!ack.message_id) {
        throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
      }

      await this.turns.eventsForMessage(request.session_id, ack.message_id, onEvent, signal);
    },
    retryEvents: async (
      request: RetryTurnRequest,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const ack = await this.turns.retry(request);
      if (!ack.message_id) {
        throw new Error(`Retry ack did not include message_id: ${ack.turn_id}`);
      }

      await this.turns.eventsForMessage(request.session_id, ack.message_id, onEvent, signal);
    },
  };

}

function createIdempotencyKey() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return `msg_${globalThis.crypto.randomUUID()}`;
  }
  return `msg_${Date.now()}_${Math.random().toString(36).slice(2)}`;
}

function normalizeTimelineEnvelope(value: unknown): TimelineEventEnvelope {
  return value as TimelineEventEnvelope;
}

function isTerminalTimelineEvent(envelope: TimelineEventEnvelope): boolean {
  return envelope.event.event_type === "turn_completed" || envelope.event.event_type === "turn_error";
}
