import type { HttpAdapter, StreamAdapter } from "./adapters";
import type {
  ModelInfo,
  SessionQuery,
  SessionUpdate,
  TalosContext,
  TalosEvent,
  TalosMessageRequest,
  TalosMessageResponse,
  TalosSession,
  ToolApprovalRequest,
} from "./types";

interface ListResponse<T> {
  data: T[];
}

interface AckResponse {
  success: boolean;
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
    create: (body: { context: TalosContext; model?: string | null }) =>
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
      const response = await this.http.request<AckResponse>(this.url(`/v1/sessions/${sessionId}`), {
        method: "DELETE",
      });
      if (!response.success) {
        throw new Error("Session close did not succeed.");
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
