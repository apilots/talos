import { afterEach, describe, expect, it, vi } from "vitest";

import { AppProtocol, createBrowserTalosClient, TalosApiError } from "./index";

const streamMocks = vi.hoisted(() => ({
  fetchEventSource: vi.fn(),
}));

vi.mock("@microsoft/fetch-event-source", async (importOriginal) => {
  const original = await importOriginal<typeof import("@microsoft/fetch-event-source")>();
  return {
    ...original,
    fetchEventSource: streamMocks.fetchEventSource,
  };
});

afterEach(() => {
  vi.unstubAllGlobals();
  streamMocks.fetchEventSource.mockReset();
});

describe("Browser files client", () => {
  it("lets FormData supply its multipart boundary", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ status: "success", data: { file: {} } }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const client = createBrowserTalosClient({ baseUrl: "http://localhost:8080" });

    await client.files.upload(
      {
        filename: "diagram.png",
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      new Uint8Array([1, 2, 3]),
    );

    const init = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect(init.body).toBeInstanceOf(FormData);
    expect(init.headers).not.toHaveProperty("Content-Type");
  });

  it("rejects HTTP 204 responses without an ApiResponse body", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(null, { status: 204 })));
    const client = createBrowserTalosClient({ baseUrl: "http://localhost:8080" });

    await expect(client.models.list()).rejects.toMatchObject({ code: AppProtocol.ErrorCode.InternalError, cause: expect.any(SyntaxError) });
  });

  it("uses XHR only when upload progress is requested", async () => {
    const onProgress = vi.fn();
    const xhr = installSuccessfulXhr();
    const client = createBrowserTalosClient({
      baseUrl: "http://localhost:8080",
      getToken: () => "token-1",
    });

    await client.files.upload(
      {
        filename: "diagram.png",
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      new Uint8Array([1, 2, 3]),
      { onProgress },
    );

    expect(xhr.open).toHaveBeenCalledWith("POST", "http://localhost:8080/v1/files");
    expect(xhr.setRequestHeader).toHaveBeenCalledWith("Authorization", "Bearer token-1");
    expect(xhr.body).toBeInstanceOf(FormData);
    expect(onProgress).toHaveBeenCalledWith({ loadedBytes: 2, totalBytes: 4 });
  });

  it("rejects progress uploads when HTTP 204 omits the ApiResponse body", async () => {
    const xhr = installPendingXhr();
    const client = createBrowserTalosClient({ baseUrl: "http://localhost:8080" });
    const upload = client.files.upload(
      {
        filename: "diagram.png",
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      new Uint8Array([1, 2, 3]),
      { onProgress: vi.fn() },
    );

    xhr.finishWith(204, "");

    await expect(upload).rejects.toMatchObject({ code: AppProtocol.ErrorCode.InternalError, cause: expect.any(SyntaxError) });
  });

  it("aborts an in-flight progress upload with the caller signal", async () => {
    const xhr = installPendingXhr();
    const controller = new AbortController();
    const client = createBrowserTalosClient({ baseUrl: "http://localhost:8080" });
    const upload = client.files.upload(
      {
        filename: "large.pdf",
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      new Uint8Array([1, 2, 3]),
      { signal: controller.signal, onProgress: vi.fn() },
    );

    controller.abort();

    await expect(upload).rejects.toMatchObject({ name: "AbortError" });
    expect(xhr.abort).toHaveBeenCalledOnce();
  });

  it("settles a progress upload when the caller signal is already aborted", async () => {
    const xhr = installPendingXhr();
    const controller = new AbortController();
    controller.abort();
    const client = createBrowserTalosClient({ baseUrl: "http://localhost:8080" });

    const upload = client.files.upload(
      {
        filename: "large.pdf",
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      new Uint8Array([1, 2, 3]),
      { signal: controller.signal, onProgress: vi.fn() },
    );

    await expect(upload).rejects.toMatchObject({ name: "AbortError" });
    expect(xhr.abort).not.toHaveBeenCalled();
    expect(xhr.body).toBeNull();
  });
});

describe("Browser timeline client", () => {
  it.each(["request", "upload", "stream"])("isolates malformed JSON diagnostics in %s", async (transport) => {
    const client = createBrowserTalosClient({ baseUrl: "http://localhost:8080" });
    let operation: Promise<unknown>;
    if (transport === "request") {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("SECRET_REMOTE_BODY")));
      operation = client.models.list();
    } else if (transport === "upload") {
      const xhr = installPendingXhr();
      operation = client.files.upload({ filename: "file.txt", scope: { type: "personal" }, tags: [], metadata: {} }, new Uint8Array(), { onProgress: vi.fn() });
      xhr.finishWith(200, "SECRET_REMOTE_BODY");
    } else {
      streamMocks.fetchEventSource.mockImplementation(async (_url: string, options: { onmessage: (event: { data: string }) => void }) => {
        options.onmessage({ data: "SECRET_REMOTE_BODY" });
      });
      operation = client.turns.events("session-1", undefined, vi.fn());
    }
    await expect(operation).rejects.toMatchObject({
      code: AppProtocol.ErrorCode.InternalError,
      message: "The backend returned invalid JSON.",
      cause: expect.any(SyntaxError),
    });
  });

  it("preserves a structured HTTP error returned instead of an event stream", async () => {
    streamMocks.fetchEventSource.mockImplementation(
      async (_url: string, options: { onopen?: (response: Response) => Promise<void> }) => {
        await options.onopen?.(
          new Response(
            JSON.stringify({
              status: "error",
              error: {
                code: AppProtocol.ErrorCode.EventCursorExpired, localization: null,
                message: "Event cursor expired",
              },
            }),
            {
              status: 410,
              statusText: "Gone",
              headers: { "Content-Type": "application/json" },
            },
          ),
        );
      },
    );
    const client = createBrowserTalosClient({ baseUrl: "http://localhost:8080" });

    const subscription = client.turns.events(
      "session-1",
      "event-1",
      vi.fn(),
    );

    await expect(subscription).rejects.toMatchObject({
      name: "TalosApiError",
      code: AppProtocol.ErrorCode.EventCursorExpired,
      message: "Event cursor expired",
    } satisfies Partial<TalosApiError>);
  });
});

interface FakeXhr {
  abort: ReturnType<typeof vi.fn>;
  body: Document | XMLHttpRequestBodyInit | null;
  emitProgress: (loaded: number, total: number) => void;
  finish: () => void;
  finishWith: (status: number, responseText: string) => void;
  open: ReturnType<typeof vi.fn>;
  setRequestHeader: ReturnType<typeof vi.fn>;
}

function installSuccessfulXhr() {
  return installPendingXhr(true);
}

function installPendingXhr(autoFinish = false): FakeXhr {
  const instance: {
    current?: {
      onabort: (() => void) | null;
      onerror: (() => void) | null;
      onload: (() => void) | null;
      upload: { onprogress: ((event: ProgressEvent) => void) | null };
    };
  } = {};
  const getInstance = () => {
    if (!instance.current) {
      throw new Error("Fake XMLHttpRequest has not been constructed");
    }
    return instance.current;
  };
  const xhr: FakeXhr = {
    abort: vi.fn(() => getInstance().onabort?.()),
    body: null,
    emitProgress: (loaded, total) =>
      getInstance().upload.onprogress?.({
        lengthComputable: true,
        loaded,
        total,
      } as ProgressEvent),
    finish: () => {
      xhr.finishWith(200, JSON.stringify({ status: "success", data: { file: {} } }));
    },
    finishWith: (status, responseText) => {
      Object.assign(getInstance(), {
        status,
        responseText,
        statusText: status === 200 ? "OK" : "No Content",
      });
      getInstance().onload?.();
    },
    open: vi.fn(),
    setRequestHeader: vi.fn(),
  };
  class FakeXmlHttpRequest {
    onabort: (() => void) | null = null;
    onerror: (() => void) | null = null;
    onload: (() => void) | null = null;
    responseText = "";
    status = 0;
    statusText = "";
    upload = { onprogress: null as ((event: ProgressEvent) => void) | null };
    abort = xhr.abort;
    open = xhr.open;
    setRequestHeader = xhr.setRequestHeader;

    constructor() {
      instance.current = this;
    }

    send(body: Document | XMLHttpRequestBodyInit | null) {
      xhr.body = body;
      if (autoFinish) {
        queueMicrotask(() => {
          xhr.emitProgress(2, 4);
          xhr.finish();
        });
      }
    }
  }
  vi.stubGlobal("XMLHttpRequest", FakeXmlHttpRequest);
  return xhr;
}

 it("connects desktop media through the SDK and sends current credentials only in the first frame",()=>{
  let token="old";const sockets:Array<EventTarget & {send:ReturnType<typeof vi.fn>}>=[];
  const constructor=vi.fn(function(this:EventTarget){const socket=Object.assign(new EventTarget(),{send:vi.fn()});sockets.push(socket);return socket;});vi.stubGlobal("WebSocket",constructor);
  const client=createBrowserTalosClient({baseUrl:"https://talos.example",getToken:()=>token});
  const grant={ticket:"ticket-secret",streamId:"stream"} as AppProtocol.DesktopViewGrant;
  client.desktop.connect(grant);token="current";sockets[0].dispatchEvent(new Event("open"));
  expect(constructor).toHaveBeenCalledWith("wss://talos.example/v1/desktop/stream");
  expect(sockets[0].send).toHaveBeenCalledExactlyOnceWith(JSON.stringify({ticket:"ticket-secret",accessToken:"current"}));
 });
