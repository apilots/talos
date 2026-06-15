export type HttpMethod = "GET" | "POST" | "PATCH" | "DELETE" | "PUT";

export interface HttpRequestOptions {
  method?: HttpMethod;
  body?: string;
  headers?: Record<string, string>;
  signal?: AbortSignal;
}

export interface StreamRequestOptions extends HttpRequestOptions {
  signal?: AbortSignal;
}

export interface HttpAdapter {
  request<T>(url: string, options?: HttpRequestOptions): Promise<T>;
}

export interface StreamAdapter {
  stream(
    url: string,
    options: StreamRequestOptions,
    onMessage: (data: string) => void,
    onError?: (error: Error) => void,
  ): Promise<void>;
}

export interface TalosHttpErrorPayload {
  status: number;
  statusText: string;
  body: string;
  url: string;
  method: string;
}

export interface TalosApiErrorBody {
  code: string;
  message: string;
}

export class TalosHttpError extends Error {
  public readonly status: number;
  public readonly statusText: string;
  public readonly body: string;
  public readonly url: string;
  public readonly method: string;
  public readonly code: string | null;
  public readonly responseMessage: string | null;

  constructor(payload: TalosHttpErrorPayload) {
    const apiError = parseTalosApiErrorBody(payload.body);
    super(apiError?.message ?? `Request failed with ${payload.status} ${payload.statusText}`);
    this.name = "TalosHttpError";
    this.status = payload.status;
    this.statusText = payload.statusText;
    this.body = payload.body;
    this.url = payload.url;
    this.method = payload.method;
    this.code = apiError?.code ?? null;
    this.responseMessage = apiError?.message ?? null;
  }
}

export function parseTalosApiErrorBody(body: string): TalosApiErrorBody | null {
  try {
    const value = JSON.parse(body) as unknown;
    if (!value || typeof value !== "object") {
      return null;
    }
    const candidate = value as Record<string, unknown>;
    if (typeof candidate.code !== "string" || typeof candidate.message !== "string") {
      return null;
    }
    return {
      code: candidate.code,
      message: candidate.message,
    };
  } catch {
    return null;
  }
}
