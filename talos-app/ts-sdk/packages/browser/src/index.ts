import { fetchEventSource } from "@microsoft/fetch-event-source";
import {
  TalosClient,
  type HttpAdapter,
  type HttpRequestOptions,
  type StreamAdapter,
  type StreamRequestOptions,
} from "@talos/sdk-core";

export * from "@talos/sdk-core";

export interface BrowserClientOptions {
  baseUrl: string;
  getToken?: () => string | null | undefined;
}

class BrowserHttpAdapter implements HttpAdapter {
  constructor(private readonly getToken?: () => string | null | undefined) {}

  async request<T>(url: string, options?: HttpRequestOptions): Promise<T> {
    const token = this.getToken?.();
    const response = await fetch(url, {
      method: options?.method ?? "GET",
      signal: options?.signal,
      headers: {
        "Content-Type": "application/json",
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(options?.headers ?? {}),
      },
      body: options?.body,
    });

    if (!response.ok) {
      const body = await response.text();
      throw new Error(`${options?.method ?? "GET"} ${url} failed with status ${response.status}: ${body}`);
    }

    if (response.status === 204) {
      return undefined as T;
    }

    return (await response.json()) as T;
  }
}

class BrowserStreamAdapter implements StreamAdapter {
  constructor(private readonly getToken?: () => string | null | undefined) {}

  async stream(
    url: string,
    options: StreamRequestOptions,
    onMessage: (data: string) => void,
    onError?: (error: Error) => void,
  ): Promise<void> {
    const token = this.getToken?.();
    try {
      await fetchEventSource(url, {
        method: options.method ?? "GET",
        body: options.body,
        signal: options.signal,
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
          ...(options.headers ?? {}),
        },
        onmessage(event) {
          if (event.data) {
            onMessage(event.data);
          }
        },
        onerror(error) {
          throw error;
        },
      });
    } catch (error) {
      const err = error instanceof Error ? error : new Error("Unknown stream error.");
      onError?.(err);
      throw err;
    }
  }
}

export function createBrowserTalosClient(options: BrowserClientOptions) {
  return new TalosClient(
    new BrowserHttpAdapter(options.getToken),
    new BrowserStreamAdapter(options.getToken),
    options.baseUrl,
  );
}
