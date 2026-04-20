import type { HttpAdapter, StreamAdapter } from "./adapters";
import type {
  DocUpdateRequest,
  MemoryFileDetail,
  MemoryFileListParams,
  MemoryFileSummary,
  MemorySearchParams,
  MemorySearchResult,
  ModelInfo,
  UserDto,
  ProjectPatch,
  ProjectRecord,
  RegisterRequest,
  LoginRequest,
  AuthResponse,
  UpdateCurrentUserRequest,
  AdminCreateUserRequest,
  AdminPatchUserRequest,
  SessionConf,
  SessionQuery,
  SessionUpdate,
  TalosContext,
  TalosEvent,
  TalosMessageRequest,
  TalosMessageResponse,
  TalosSession,
  ToolApprovalRequest,
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
      list: async (query?: SessionQuery): Promise<TalosSession[]> => {
        const response = await this.http.request<{ data: TalosSession[] }>(
          this.urlWithQuery("/v1/admin/sessions", query),
        );
        return response.data;
      },
    },
    projects: {
      list: async (): Promise<ProjectRecord[]> => {
        const response = await this.http.request<{ data: ProjectRecord[] }>(this.url("/v1/admin/projects"));
        return response.data;
      },
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
    turns: async (sessionId: string): Promise<TalosMessageResponse[]> => {
      const response = await this.http.request<ListResponse<TalosMessageResponse>>(
        this.url(`/v1/sessions/${sessionId}/turns`),
      );
      return response.data;
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
      this.http.request<TalosMessageResponse>(this.url(`/v1/sessions/${sessionId}/messages`), {
        method: "POST",
        body: JSON.stringify(request),
      }),
    sendStream: async (
      sessionId: string,
      request: TalosMessageRequest,
      onEvent: (event: TalosEvent) => void,
      signal?: AbortSignal,
    ): Promise<void> => {
      const streamRequest: TalosMessageRequest = {
        ...request,
        stream: true,
      };
      await this.streamAdapter.stream(
        this.url(`/v1/sessions/${sessionId}/messages`),
        {
          method: "POST",
          body: JSON.stringify(streamRequest),
          signal,
        },
        (data) => {
          if (data === "[DONE]") {
            return;
          }
          onEvent(JSON.parse(data) as TalosEvent);
        },
      );
    },
  };

  toolApprovals = {
    submit: (sessionId: string, request: ToolApprovalRequest) =>
      this.http.request<void>(this.url(`/v1/sessions/${sessionId}/tool-approvals`), {
        method: "POST",
        body: JSON.stringify(request),
      }),
  };
}
