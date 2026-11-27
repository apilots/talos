import {
  EventStreamContentType,
  fetchEventSource,
} from "@microsoft/fetch-event-source";
import {
  AppProtocol,
  TalosClient,
  TalosApiError,
  type HttpAdapter,
  type DesktopStreamAdapter,
  type DesktopViewGrant,
  type HttpRequestOptions,
  parseTalosApiErrorBody,
  parseTalosJson,
  TalosHttpError,
  type StreamAdapter,
  type StreamRequestOptions,
  unwrapTalosApiResponse,
} from "@talos/sdk-core";

export * from "@talos/sdk-core";

export interface BrowserClientOptions {
  baseUrl: string;
  getToken?: () => string | null | undefined;
  /** Signed-in actor to verify when a restarted Server requires admission. */
  getUserId?: () => string | null | undefined;
  /** Use the same host transport for JSON, multipart uploads and event streams. */
  fetch?: typeof globalThis.fetch;
}

type HttpResponse = Pick<Response, "ok" | "status" | "statusText" | "text">;

async function readJson<T>(response: HttpResponse, url: string, method: string): Promise<T> {
  const body = await response.text();
  if (!response.ok) {
    throw httpFailure({ method, status: response.status, statusText: response.statusText, body, url });
  }
  return unwrapTalosApiResponse<T>(parseTalosJson(body));
}

/** Bind one operation, including visibility reconnects, to its original credentials. */
class BrowserServerSession {
  readonly token: string | null | undefined;
  private readonly userId: string | null | undefined;
  private readonly userUrl: string;
  private readonly fetchServer: typeof globalThis.fetch;

  constructor(private readonly options: BrowserClientOptions) {
    this.token = options.getToken?.();
    this.userId = options.getUserId?.();
    this.userUrl = options.baseUrl.replace(/\/$/, "") + "/v1/user";
    this.fetchServer = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  fetch(url: string, init: RequestInit, onUploadProgress?: HttpRequestOptions["onUploadProgress"]) {
    return this.withBootstrap(url, init.signal, () => this.fetchServer(url, {
      ...(init.body instanceof FormData && onUploadProgress ? uploadRequest(url, init, onUploadProgress) : init),
      redirect: "error",
    }));
  }

  async withBootstrap<T extends HttpResponse>(url: string, signal: AbortSignal | null | undefined, send: () => Promise<T>): Promise<T> {
    this.assertSameActor();
    const response = await send();
    if (response.status !== 401 || !this.token || !this.userId || url === this.userUrl) return response;

    await response.text();
    this.assertSameActor();
    const bootstrap = await this.fetchServer(this.userUrl, {
      method: "GET", headers: { Authorization: `Bearer ${this.token}` }, signal, redirect: "error",
    });
    const identity = await readJson<AppProtocol.UserCurrentResponse>(bootstrap, this.userUrl, "GET");
    this.assertSameActor(identity?.user?.user_id === this.userId);
    return send();
  }

  private assertSameActor(matches = true) {
    if (!matches || this.options.getToken?.() !== this.token || this.options.getUserId?.() !== this.userId) {
      throw new TalosApiError({ code: AppProtocol.ErrorCode.PermissionDenied, message: "The signed-in account has changed.", localization: null });
    }
  }
}

class BrowserHttpAdapter implements HttpAdapter {
  constructor(private readonly options: BrowserClientOptions) {}

  async request<T>(url: string, options?: HttpRequestOptions): Promise<T> {
    const session = new BrowserServerSession(this.options);
    const isMultipart = options?.body instanceof FormData;
    const method = options?.method ?? "GET";
    const response = isMultipart && options?.onUploadProgress && !this.options.fetch
      ? await session.withBootstrap(url, options.signal, () => xhrRequest(url, options, session.token))
      : await session.fetch(url, {
        method, signal: options?.signal,
        headers: {
          ...(!isMultipart ? { "Content-Type": "application/json" } : {}),
          ...(session.token ? { Authorization: `Bearer ${session.token}` } : {}),
          ...(options?.headers ?? {}),
        },
        body: options?.body,
      }, options?.onUploadProgress);
    return readJson<T>(response, url, method);
  }
}

/** Count encoded multipart bytes as the selected transport consumes them, without buffering the upload. */
function uploadRequest(url: string, init: RequestInit, onProgress: NonNullable<HttpRequestOptions["onUploadProgress"]>) {
  const request = new Request(url, init);
  const reader = request.body!.getReader();
  let loadedBytes = 0;
  const body = new ReadableStream<Uint8Array>({
    async pull(controller) {
      try {
        const { value, done } = await reader.read();
        if (done) {
          controller.close();
          onProgress({ loadedBytes, totalBytes: loadedBytes });
        } else {
          loadedBytes += value.byteLength;
          controller.enqueue(value);
          onProgress({ loadedBytes, totalBytes: null });
        }
      } catch (error) {
        controller.error(error);
      }
    },
    cancel: reason => reader.cancel(reason),
  }, { highWaterMark: 0 });
  return { ...init, headers: request.headers, body, duplex: "half" as const };
}

function xhrRequest(
  url: string,
  options: HttpRequestOptions,
  token: string | null | undefined,
): Promise<HttpResponse> {
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
      finish(() => resolve({
        ok: xhr.status >= 200 && xhr.status < 300,
        status: xhr.status,
        statusText: xhr.statusText,
        text: () => Promise.resolve(xhr.responseText),
      }));
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
  constructor(private readonly options: BrowserClientOptions) {}

  async stream(
    url: string,
    options: StreamRequestOptions,
    onMessage: (data: string, event?: string) => void,
    onError?: (error: Error) => void,
  ): Promise<void> {
    const session = new BrowserServerSession(this.options);
    try {
      await fetchEventSource(url, {
        fetch: (_input, init) => {
          const requestUrl = new URL(url);
          const lastEventId = new Headers(init?.headers).get("last-event-id");
          // The gateway prefers the query cursor over Last-Event-ID on reconnect.
          if (lastEventId) requestUrl.searchParams.set("after_event_id", lastEventId);
          return session.fetch(requestUrl.toString(), init ?? {});
        },
        method: options.method ?? "GET",
        body: options.body,
        signal: options.signal,
        headers: {
          "Content-Type": "application/json",
          ...(session.token ? { Authorization: `Bearer ${session.token}` } : {}),
          ...(options.headers ?? {}),
        },
        async onopen(response) {
          const contentType = response.headers.get("content-type");
          if (response.ok && contentType?.startsWith(EventStreamContentType)) {
            options.onOpen?.();
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
            onMessage(event.data, event.event);
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
    new BrowserHttpAdapter(options),
    new BrowserStreamAdapter(options),
    options.baseUrl,
    new BrowserDesktopStreamAdapter(options),
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

/** Admission credentials are sent once in the first frame, never in a URL. */
class BrowserDesktopStreamAdapter implements DesktopStreamAdapter {
  constructor(private readonly options: BrowserClientOptions) {}
  connect(grant: DesktopViewGrant): WebSocket {
    const url = `${this.options.baseUrl.replace(/\/$/, "")}/v1/desktop/stream`.replace(/^http/, "ws");
    const socket = new WebSocket(url);
    socket.addEventListener("open", () => socket.send(JSON.stringify({ ticket: grant.ticket, accessToken: this.options.getToken?.() })), { once: true });
    return socket;
  }
}
