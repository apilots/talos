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
