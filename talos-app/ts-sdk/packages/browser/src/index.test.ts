import { afterEach, describe, expect, it, vi } from "vitest";

import { createBrowserTalosClient, TalosHttpError } from "./index";

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
      new Response(JSON.stringify({ file: {} }), {
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
  it("preserves a structured HTTP error returned instead of an event stream", async () => {
    streamMocks.fetchEventSource.mockImplementation(
      async (_url: string, options: { onopen?: (response: Response) => Promise<void> }) => {
        await options.onopen?.(
          new Response(
            JSON.stringify({
              code: "event_cursor_expired",
              message: "event cursor is older than retained timeline events",
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
      name: "TalosHttpError",
      status: 410,
      code: "event_cursor_expired",
      message: "event cursor is older than retained timeline events",
    } satisfies Partial<TalosHttpError>);
  });
});

interface FakeXhr {
  abort: ReturnType<typeof vi.fn>;
  body: Document | XMLHttpRequestBodyInit | null;
  emitProgress: (loaded: number, total: number) => void;
  finish: () => void;
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
      Object.assign(getInstance(), {
        status: 200,
        responseText: JSON.stringify({ file: {} }),
        statusText: "OK",
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
