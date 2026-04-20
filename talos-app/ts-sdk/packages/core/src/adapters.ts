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

export class TalosHttpError extends Error {
  public readonly status: number;
  public readonly statusText: string;
  public readonly body: string;
  public readonly url: string;
  public readonly method: string;

  constructor(payload: TalosHttpErrorPayload) {
    super(`${payload.method} ${payload.url} failed with ${payload.status} ${payload.statusText}`);
    this.name = "TalosHttpError";
    this.status = payload.status;
    this.statusText = payload.statusText;
    this.body = payload.body;
    this.url = payload.url;
    this.method = payload.method;
  }
}
