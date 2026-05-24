import type { HttpAdapter, StreamAdapter } from "./adapters";
import type {
  DocUpdateRequest,
  LlmConfig,
  LlmConfigRead,
  MemoryFileDetail,
  MemoryFileListParams,
  MemoryFileSummary,
  MemorySearchParams,
  MemorySearchResult,
  ModelInfo,
  UserDto,
  UserTokenMeta,
  ProjectPatch,
  ProjectRecord,
  PaginatedResponse,
  RegisterRequest,
  LoginRequest,
  AuthResponse,
  CreateUserTokenRequest,
  CreateUserTokenResult,
  UpdateCurrentUserRequest,
  AdminCreateUserRequest,
  AdminPatchUserRequest,
  SessionConf,
  SessionQuery,
  SessionUpdate,
  TalosContext,
  TalosUsage,
  TimelineEventEnvelope,
  TalosActiveTurn,
  TalosSessionHistorySnapshot,
  TalosMessageAck,
  TalosMessageRequest,
  TalosMessageRetryRequest,
  TalosSession,
  TalosTurnExecutionMetrics,
  TimelineRecoverySnapshot,
  WorkspaceDocDetail,
  WorkspaceDocKind,
  WorkspaceDocSummary,
} from "./types";

interface ListResponse<T> {
  data: T[];
}

interface AckResponse {
  success: boolean;
}

interface CreateSessionRequest {
  context: TalosContext;
  model?: string | null;
  conf?: SessionConf | null;
  project_id?: string | null;
  current_directory?: string | null;
}

interface CreateProjectRequest {
  name: string;
  directory: string;
}

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
      if (value !== undefined) {
        params.set(key, String(value));
      }
    }
    const serialized = params.toString();
    return serialized ? `${this.url(path)}?${serialized}` : this.url(path);
  }

  auth = {
    register: async (request: RegisterRequest): Promise<AuthResponse> => {
      return this.http.request<AuthResponse>(this.url("/v1/auth/register"), {
        method: "POST",
        body: JSON.stringify(request),
      });
    },
    login: async (request: LoginRequest): Promise<AuthResponse> => {
      return this.http.request<AuthResponse>(this.url("/v1/auth/login"), {
        method: "POST",
        body: JSON.stringify(request),
      });
    },
    logout: async (): Promise<void> => {
      await this.http.request<void>(this.url("/v1/auth/logout"), {
        method: "POST",
      });
    },
  };

  user = {
    current: async (): Promise<UserDto> => {
      return this.http.request<UserDto>(this.url("/v1/user"));
    },
    update: async (request: UpdateCurrentUserRequest): Promise<UserDto> => {
      return this.http.request<UserDto>(this.url("/v1/user"), {
        method: "PATCH",
        body: JSON.stringify(request),
      });
    },
    tokens: {
      list: async (): Promise<UserTokenMeta[]> => {
        const response = await this.http.request<ListResponse<UserTokenMeta>>(this.url("/v1/user/tokens"));
        return response.data;
      },
      create: async (request: CreateUserTokenRequest): Promise<CreateUserTokenResult> => {
        return this.http.request<CreateUserTokenResult>(this.url("/v1/user/tokens"), {
          method: "POST",
          body: JSON.stringify(request),
        });
      },
      delete: async (tokenId: string): Promise<void> => {
        await this.http.request<void>(this.url(`/v1/user/tokens/${tokenId}`), {
          method: "DELETE",
        });
      },
    },
  };

  admin = {
    users: {
      list: async (): Promise<UserDto[]> => {
        const response = await this.http.request<{ data: UserDto[] }>(this.url("/v1/admin/users"));
        return response.data;
      },
      create: async (request: AdminCreateUserRequest): Promise<UserDto> => {
        return this.http.request<UserDto>(this.url("/v1/admin/users"), {
          method: "POST",
          body: JSON.stringify(request),
        });
      },
      patch: async (userId: string, request: AdminPatchUserRequest): Promise<UserDto> => {
        return this.http.request<UserDto>(this.url(`/v1/admin/users/${userId}`), {
          method: "PATCH",
          body: JSON.stringify(request),
        });
      },
      delete: async (userId: string): Promise<void> => {
        await this.http.request<void>(this.url(`/v1/admin/users/${userId}`), {
          method: "DELETE",
        });
      },
    },
    sessions: {
      list: async (query?: SessionQuery): Promise<PaginatedResponse<TalosSession>> => {
        return this.http.request<PaginatedResponse<TalosSession>>(
          this.urlWithQuery("/v1/admin/sessions", query),
        );
      },
      delete: async (sessionId: string): Promise<void> => {
        await this.http.request<void>(this.url(`/v1/admin/sessions/${sessionId}`), {
          method: "DELETE",
        });
      },
    },
    projects: {
      list: async (): Promise<ProjectRecord[]> => {
        const response = await this.http.request<{ data: ProjectRecord[] }>(this.url("/v1/admin/projects"));
        return response.data;
      },
    },
  };

  llm = {
    get: (): Promise<LlmConfigRead> => {
      return this.http.request<LlmConfigRead>(this.url("/v1/llm"));
    },
    update: (config: LlmConfig): Promise<LlmConfigRead> => {
      return this.http.request<LlmConfigRead>(this.url("/v1/llm"), {
        method: "PUT",
        body: JSON.stringify(config),
      });
    },
  };

  models = {
    list: async (): Promise<ModelInfo[]> => {
      const response = await this.http.request<ListResponse<ModelInfo>>(this.url("/v1/models"));
      return response.data;
    },
  };

  sessions = {
    list: async (query?: SessionQuery): Promise<TalosSession[]> => {
      const response = await this.http.request<ListResponse<TalosSession>>(
        this.urlWithQuery("/v1/sessions", query),
      );
      return response.data;
    },
    create: (body: CreateSessionRequest) =>
      this.http.request<TalosSession>(this.url("/v1/sessions"), {
        method: "POST",
        body: JSON.stringify(body),
      }),
    get: (sessionId: string) =>
      this.http.request<TalosSession>(this.url(`/v1/sessions/${sessionId}`)),
    patch: async (sessionId: string, update: SessionUpdate): Promise<void> => {
      const response = await this.http.request<AckResponse>(this.url(`/v1/sessions/${sessionId}`), {
        method: "PATCH",
        body: JSON.stringify(update),
      });
      if (!response.success) {
        throw new Error("Session patch did not succeed.");
      }
    },
    close: async (sessionId: string): Promise<void> => {
      const response = await this.http.request<AckResponse>(this.url(`/v1/sessions/${sessionId}/close`), {
        method: "POST",
      });
      if (!response.success) {
        throw new Error("Session close did not succeed.");
      }
    },
    delete: async (sessionId: string): Promise<void> => {
      const response = await this.http.request<AckResponse>(this.url(`/v1/sessions/${sessionId}`), {
        method: "DELETE",
      });
      if (!response.success) {
        throw new Error("Session delete did not succeed.");
      }
    },
    cancel: async (sessionId: string): Promise<void> => {
      const response = await this.http.request<AckResponse>(
        this.url(`/v1/sessions/${sessionId}/cancel`),
        { method: "POST" },
      );
      if (!response.success) {
        throw new Error("Session cancel did not succeed.");
      }
    },
    compact: (sessionId: string) =>
      this.http.request<TalosSession>(this.url(`/v1/sessions/${sessionId}/compact`), {
        method: "POST",
      }),
    clear: (sessionId: string) =>
      this.http.request<TalosSession>(this.url(`/v1/sessions/${sessionId}/clear`), {
        method: "POST",
      }),
    activeTurn: (sessionId: string) =>
      this.http.request<TalosActiveTurn>(this.url(`/v1/sessions/${sessionId}/active-turn`)),
    history: async (sessionId: string): Promise<TalosSessionHistorySnapshot> => {
      const response = await this.http.request<TalosSessionHistorySnapshot>(
        this.url(`/v1/sessions/${sessionId}/history`),
      );
      return normalizeSessionHistorySnapshot(response);
    },
    recover: async (
      sessionId: string,
      afterEventId?: string,
      replayLimit?: number,
    ): Promise<TimelineRecoverySnapshot> => {
      const response = await this.http.request<TimelineRecoverySnapshot>(
        this.urlWithQuery(`/v1/sessions/${sessionId}/recovery`, {
          after_event_id: afterEventId,
          limit: replayLimit,
        }),
      );
      return normalizeTimelineRecoverySnapshot(response);
    },
  };

  projects = {
    list: async (): Promise<ProjectRecord[]> => {
      const response = await this.http.request<ListResponse<ProjectRecord>>(this.url("/v1/projects"));
      return response.data;
    },
    get: (projectId: string) =>
      this.http.request<ProjectRecord>(this.url(`/v1/projects/${projectId}`)),
    create: (body: CreateProjectRequest) =>
      this.http.request<ProjectRecord>(this.url("/v1/projects"), {
        method: "POST",
        body: JSON.stringify(body),
      }),
    patch: (projectId: string, patch: ProjectPatch) =>
      this.http.request<ProjectRecord>(this.url(`/v1/projects/${projectId}`), {
        method: "PATCH",
        body: JSON.stringify(patch),
      }),
    delete: async (projectId: string): Promise<void> => {
      await this.http.request<void>(this.url(`/v1/projects/${projectId}`), {
        method: "DELETE",
      });
    },
  };

  memory = {
    list: async (query?: MemoryFileListParams): Promise<MemoryFileSummary[]> => {
      const response = await this.http.request<ListResponse<MemoryFileSummary>>(
        this.urlWithQuery("/v1/memory/files", query),
      );
      return response.data;
    },
    get: (date: string) =>
      this.http.request<MemoryFileDetail>(this.url(`/v1/memory/files/${date}`)),
    put: (date: string, request: DocUpdateRequest) =>
      this.http.request<MemoryFileDetail>(this.url(`/v1/memory/files/${date}`), {
        method: "PUT",
        body: JSON.stringify(request),
      }),
    search: async (
      query: MemorySearchParams,
      options?: RequestControlOptions,
    ): Promise<MemorySearchResult[]> => {
      const requestOptions = options?.signal ? { signal: options.signal } : undefined;
      const url = this.urlWithQuery("/v1/memory/search", query);
      const response = requestOptions
        ? await this.http.request<ListResponse<MemorySearchResult>>(url, requestOptions)
        : await this.http.request<ListResponse<MemorySearchResult>>(url);
      return response.data;
    },
  };

  workspaceDocs = {
    list: async (): Promise<WorkspaceDocSummary[]> => {
      const response = await this.http.request<ListResponse<WorkspaceDocSummary>>(
        this.url("/v1/workspace/docs"),
      );
      return response.data;
    },
    get: (kind: WorkspaceDocKind) =>
      this.http.request<WorkspaceDocDetail>(this.url(`/v1/workspace/docs/${kind}`)),
    put: (kind: WorkspaceDocKind, request: DocUpdateRequest) =>
      this.http.request<WorkspaceDocDetail>(this.url(`/v1/workspace/docs/${kind}`), {
        method: "PUT",
        body: JSON.stringify(request),
      }),
  };

  messages = {
    send: (sessionId: string, request: TalosMessageRequest) =>
      this.messages.sendWithIdempotencyKey(sessionId, request, createIdempotencyKey()),
    sendWithIdempotencyKey: (
      sessionId: string,
      request: TalosMessageRequest,
      idempotencyKey: string,
    ) =>
      this.http.request<TalosMessageAck>(this.url(`/v1/sessions/${sessionId}/messages`), {
        method: "POST",
        headers: {
          "idempotency-key": idempotencyKey,
        },
        body: JSON.stringify(request),
      }),
    retryWithIdempotencyKey: (
      sessionId: string,
      request: TalosMessageRetryRequest,
      idempotencyKey: string,
    ) =>
      this.http.request<TalosMessageAck>(this.url(`/v1/sessions/${sessionId}/message-attempts`), {
        method: "POST",
        headers: {
          "idempotency-key": idempotencyKey,
        },
        body: JSON.stringify(request),
      }),
    retry: (sessionId: string, request: TalosMessageRetryRequest) =>
      this.messages.retryWithIdempotencyKey(
        sessionId,
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
        await this.messages.events(
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
    sendEvents: async (
      sessionId: string,
      request: TalosMessageRequest,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const ack = await this.messages.send(sessionId, request);
      if (!ack.message_id) {
        throw new Error(`Message ack did not include message_id: ${ack.turn_id}`);
      }

      await this.messages.eventsForMessage(sessionId, ack.message_id, onEvent, signal);
    },
    retryEvents: async (
      sessionId: string,
      request: TalosMessageRetryRequest,
      onEvent: (event: TimelineEventEnvelope) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const ack = await this.messages.retry(sessionId, request);
      if (!ack.message_id) {
        throw new Error(`Retry ack did not include message_id: ${ack.turn_id}`);
      }

      await this.messages.eventsForMessage(sessionId, ack.message_id, onEvent, signal);
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
  const envelope = value as TimelineEventEnvelope;
  const event = envelope.event;
  return {
    ...envelope,
    event_seq: Number(envelope.event_seq),
    created_at_ms: Number(envelope.created_at_ms),
    event: {
      ...event,
      event_seq: event.event_seq == null ? event.event_seq : Number(event.event_seq),
      created_at_ms: event.created_at_ms == null ? event.created_at_ms : Number(event.created_at_ms),
      sequence: Number(event.sequence),
      payload: normalizeTimelinePayload(event.payload),
    },
  };
}

function normalizeTimelinePayload(
  payload: TimelineEventEnvelope["event"]["payload"],
): TimelineEventEnvelope["event"]["payload"] {
  if (payload.type === "turn_metrics") {
    return {
      ...payload,
      metrics: normalizeTurnExecutionMetrics(payload.metrics)!,
    };
  }
  if (payload.type === "turn_completed") {
    return {
      ...payload,
      completed_at_ms: Number(payload.completed_at_ms),
      usage: normalizeUsage(payload.usage),
      metrics: normalizeTurnExecutionMetrics(payload.metrics),
    };
  }
  return payload;
}

function normalizeTimelineRecoverySnapshot(
  snapshot: TimelineRecoverySnapshot,
): TimelineRecoverySnapshot {
  return {
    ...snapshot,
    replay: {
      ...snapshot.replay,
      events: snapshot.replay.events.map((event) => normalizeTimelineEnvelope(event)),
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

function isTerminalTimelineEvent(envelope: TimelineEventEnvelope): boolean {
  return envelope.event.event_type === "turn_completed" || envelope.event.event_type === "turn_error";
}
