import {
  EventStreamContentType,
  fetchEventSource,
} from "@microsoft/fetch-event-source";
import {
  TalosClient,
  TalosApiError,
  type HttpAdapter,
  type HttpRequestOptions,
  parseTalosApiErrorBody,
  TalosHttpError,
  type StreamAdapter,
  type StreamRequestOptions,
  unwrapTalosApiResponse,
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
    const isMultipart = options?.body instanceof FormData;
    if (isMultipart && options?.onUploadProgress) {
      return xhrRequest(url, options, token);
    }
    const response = await fetch(url, {
      method: options?.method ?? "GET",
      signal: options?.signal,
      headers: {
        ...(!isMultipart ? { "Content-Type": "application/json" } : {}),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(options?.headers ?? {}),
      },
      body: options?.body,
    });

    if (!response.ok) {
      const body = await response.text();
      throw httpFailure({
        method: options?.method ?? "GET",
        status: response.status,
        statusText: response.statusText,
        body,
        url,
      });
    }

    return unwrapTalosApiResponse<T>(await response.json());
  }
}

function xhrRequest<T>(
  url: string,
  options: HttpRequestOptions,
  token: string | null | undefined,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const method = options.method ?? "GET";
    const xhr = new XMLHttpRequest();
    const signal = options.signal;
    const abort = () => xhr.abort();
    const finish = (result: () => void) => {
      signal?.removeEventListener("abort", abort);
      result();
    };

    xhr.open(method, url);
    if (token) {
      xhr.setRequestHeader("Authorization", `Bearer ${token}`);
    }
    for (const [name, value] of Object.entries(options.headers ?? {})) {
      xhr.setRequestHeader(name, value);
    }
    xhr.upload.onprogress = (event) => {
      options.onUploadProgress?.({
        loadedBytes: event.loaded,
        totalBytes: event.lengthComputable ? event.total : null,
      });
    };
    xhr.onload = () => {
      finish(() => {
        if (xhr.status < 200 || xhr.status >= 300) {
          reject(httpFailure({
            method,
            status: xhr.status,
            statusText: xhr.statusText,
            body: xhr.responseText,
            url,
          }));
          return;
        }
        try {
          resolve(unwrapTalosApiResponse<T>(JSON.parse(xhr.responseText) as unknown));
        } catch (error) {
          reject(error);
        }
      });
    };
    xhr.onerror = () => finish(() => reject(new TypeError("Network request failed.")));
    xhr.onabort = () =>
      finish(() => reject(new DOMException("The operation was aborted.", "AbortError")));
    if (signal?.aborted) {
      finish(() => reject(abortError(signal)));
      return;
    }
    signal?.addEventListener("abort", abort, { once: true });
    xhr.send(options.body ?? null);
  });
}

function abortError(signal: AbortSignal): Error {
  return signal.reason instanceof Error
    ? signal.reason
    : new DOMException("The operation was aborted.", "AbortError");
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
        async onopen(response) {
          const contentType = response.headers.get("content-type");
          if (response.ok && contentType?.startsWith(EventStreamContentType)) {
            return;
          }
          const body = await response.text();
          throw httpFailure({
            method: options.method ?? "GET",
            status: response.status,
            statusText: response.statusText,
            body,
            url,
          });
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

function httpFailure(payload: {
  status: number;
  statusText: string;
  body: string;
  url: string;
  method: string;
}): Error {
  const apiError = parseTalosApiErrorBody(payload.body);
  if (apiError) {
    return new TalosApiError(apiError);
  }
  return new TalosHttpError(payload);
}
