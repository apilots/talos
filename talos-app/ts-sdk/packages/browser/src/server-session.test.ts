import { fetchEventSource } from "@microsoft/fetch-event-source";
import { describe, expect, it, vi } from "vitest";
import { AppProtocol, createBrowserTalosClient } from "./index";

vi.mock("@microsoft/fetch-event-source", { spy: true });

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
    expect(request.mock.calls[0][1]?.body).toBeInstanceOf(FormData);
  });

  it("reopens SSE with its original cursor and latest Last-Event-ID through the selected fetch", async () => {
    const request = vi.fn<typeof fetch>().mockResolvedValueOnce(unauthorized())
      .mockResolvedValueOnce(success({ user: { user_id: getUserId() } }))
      .mockResolvedValueOnce(new Response(null, { headers: { "content-type": "text/event-stream" } }));
    vi.mocked(fetchEventSource).mockImplementation(async (url, options) => {
      const response = await options.fetch!(url, { headers: { ...options.headers, "last-event-id": "live-id" }, signal: options.signal });
      await options.onopen?.(response);
    });
    const client = createBrowserTalosClient({ baseUrl, getToken, getUserId, fetch: request });
    const onOpen = vi.fn();
    await client.turns.events("session", "saved-id", vi.fn(), undefined, { onOpen });
    expect(onOpen).toHaveBeenCalledOnce();
    expect(request.mock.calls[2]).toEqual(request.mock.calls[0]);
    expect(request.mock.calls[2][0]).toBe(baseUrl + "/v1/sessions/session/events?after_event_id=saved-id");
    expect(new Headers(request.mock.calls[2][1]?.headers).get("last-event-id")).toBe("live-id");
  });
});
