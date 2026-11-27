import { afterEach, describe, expect, it, vi } from "vitest";
import { AppProtocol, createBrowserTalosClient, type TimelineEvent } from "./index";

afterEach(() => vi.unstubAllGlobals());

describe("authenticated Server recovery", () => {
  const baseUrl = "talos-cloud://server";
  const getToken = () => "session-token";
  const getUserId = () => "account-a";
  const success = (data: unknown) => Response.json({ status: "success", data });
  const unauthorized = () => new Response(null, { status: 401, statusText: "Unauthorized" });

  it("replays the identical mutation once after the expected Server user is admitted", async () => {
    const request = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(unauthorized())
      .mockResolvedValueOnce(success({ user: { user_id: getUserId() } }))
      .mockResolvedValueOnce(success({ turnId: "turn-1" }));
    const client = createBrowserTalosClient({ baseUrl, getToken, getUserId, fetch: request });
    const turn: AppProtocol.StartTurnRequest = {
      sessionId: "session", input: { content: [{ type: "text", text: "hello" }] },
      userPermissionMode: null, modelSelection: null, projectId: null, currentDirectory: null, clientRequestId: null,
    };
    await expect(client.turns.startWithIdempotencyKey(turn, "one-turn")).resolves.toMatchObject({ turnId: "turn-1" });
    expect(request.mock.calls.map(([url]) => url)).toEqual([
      baseUrl + "/v1/turns/start", baseUrl + "/v1/user", baseUrl + "/v1/turns/start",
    ]);
    expect(request.mock.calls[2]).toEqual(request.mock.calls[0]);
    expect(request.mock.calls[0][1]?.headers).toMatchObject({ "idempotency-key": "one-turn" });
    for (const [, init] of request.mock.calls) {
      expect(init?.redirect).toBe("error");
      expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer session-token");
    }
  });

  it("sends multipart progress uploads through an explicitly selected transport", async () => {
    const request = vi.fn<typeof fetch>().mockResolvedValueOnce(success({ file: {} }));
    const client = createBrowserTalosClient({ baseUrl, getToken, getUserId, fetch: request });
    await client.files.upload({ filename: "file.txt", scope: { type: "personal" }, tags: [], metadata: {} },
      new Uint8Array([1]), { onProgress: vi.fn() });
    expect(request.mock.calls[0][1]?.body).toBeInstanceOf(ReadableStream);
  });

  it.each(["https://server.example", baseUrl])("delivers each SSE event once across visibility and Server recovery at %s", async (serverUrl) => {
    const document = Object.assign(new EventTarget(), { hidden: false });
    vi.stubGlobal("document", document);
    vi.stubGlobal("window", { clearTimeout, setTimeout });
    let streamRequests = 0;
    const lastEventIds: Array<string | null> = [];
    const request = vi.fn<typeof fetch>( (input, init) => {
      const url = new URL((input instanceof Request ? input.url : input.toString()));
      if (url.pathname === "/v1/user") return Promise.resolve(success({ user: { user_id: getUserId() } }));
      lastEventIds.push(new Headers(init?.headers).get("last-event-id"));
      if (++streamRequests === 2) return Promise.resolve(unauthorized());

      // Match the gateway's query-first cursor selection during replay.
      const cursor = url.searchParams.get("after_event_id") ?? new Headers(init?.headers).get("last-event-id");
      const lastSequence = Number(cursor?.slice(3) ?? 0);
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          init?.signal?.addEventListener("abort", () => controller.error(init.signal?.reason), { once: true });
          for (let sequence = lastSequence + 1; sequence <= (streamRequests === 1 ? 3 : 4); sequence++) {
            const event: TimelineEvent = {
              eventId: `ev_${sequence}`, eventSeq: sequence, persistedAt: sequence,
              event: { type: "agent", data: {
                identity: { sessionId: "session", turnId: "turn" }, sequence, occurredAt: sequence,
                payload: { type: "turnStarted", data: {} },
              } },
            };
            controller.enqueue(new TextEncoder().encode(`id: ${event.eventId}\ndata: ${JSON.stringify(event)}\n\n`));
          }
        },
      });
      return Promise.resolve(new Response(body, { headers: { "content-type": "text/event-stream" } }));
    });
    const client = createBrowserTalosClient({ baseUrl: serverUrl, getToken, getUserId, fetch: request });
    const controller = new AbortController();
    const onOpen = vi.fn();
    const delivered: string[] = [];
    const subscription = client.turns.events("session", "ev_1", (event) => delivered.push(event.eventId), controller.signal, { onOpen, persistent: true });
    try {
      await vi.waitFor(() => expect(delivered).toEqual(["ev_2", "ev_3"]));
      document.hidden = true;
      document.dispatchEvent(new Event("visibilitychange"));
      // Browser visibility changes arrive in separate tasks after abort settles.
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
      expect(request.mock.calls[0][1]?.signal?.aborted).toBe(true);
      document.hidden = false;
      document.dispatchEvent(new Event("visibilitychange"));
      await vi.waitFor(() => expect(delivered).toContain("ev_4"));

      expect(delivered).toEqual(["ev_2", "ev_3", "ev_4"]);
      expect(onOpen).toHaveBeenCalledTimes(2);
      expect(request.mock.calls[0][0]).toBe(serverUrl + "/v1/sessions/session/events?after_event_id=ev_1&close_when_idle=false");
      expect(request.mock.calls[3]).toEqual(request.mock.calls[1]);
      expect(new URL(((input) => input instanceof Request ? input.url : input.toString())(request.mock.calls[3][0])).searchParams.get("after_event_id")).toBe("ev_3");
      expect(lastEventIds).toEqual([null, "ev_3", "ev_3"]);
    } finally {
      controller.abort();
      await subscription;
    }
  });
});
