import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";
import type { HttpAdapter, StreamAdapter } from "../adapters";

describe("TalosClient", () => {
  it("lists the account Computer directory with unchanged filters and optional cancellation", async () => {
    const directory = { data: [{ nodeId: "worker-1" }], nextCursor: null, total: 1 };
    const request = vi.fn().mockResolvedValue(directory);
    const client = new TalosClient({ request }, noopStreamAdapter(), "http://localhost:8080/");
    const controller = new AbortController();

    expect(await client.computers.list()).toEqual(directory);
    expect(request).toHaveBeenNthCalledWith(1, "http://localhost:8080/v1/computers");
    expect(await client.computers.list({ query: "My Worker", cursor: "next/page", limit: 20 }, controller.signal)).toEqual(directory);
    expect(request).toHaveBeenNthCalledWith(
      2,
      "http://localhost:8080/v1/computers?query=My+Worker&cursor=next%2Fpage&limit=20",
      { signal: controller.signal },
    );
  });

  it("propagates cancellation of a pending Computer list to the HTTP adapter", async () => {
    const request = vi.fn().mockImplementation((_url, options) => new Promise((_resolve, reject) => {
      options.signal.addEventListener("abort", () => reject(options.signal.reason), { once: true });
    }));
    const client = new TalosClient({ request }, noopStreamAdapter(), "http://localhost:8080");
    const controller = new AbortController();
    const pending = client.computers.list({}, controller.signal);
    const aborted = expect(pending).rejects.toMatchObject({ name: "AbortError" });

    controller.abort();

    await aborted;
  });

  it("reads and writes llm config through the user config route", async () => {
    const request = vi.fn().mockResolvedValue({ provider: "anthropic", model: "claude-sonnet-4" });
    const client = new TalosClient({ request }, noopStreamAdapter(), "http://localhost:8080/");
    const config = { provider: "anthropic", model: "claude-sonnet-4" } as never;

    await client.llm.get();
    await client.llm.update(config);

    expect(request).toHaveBeenNthCalledWith(1, "http://localhost:8080/v1/llm");
    expect(request).toHaveBeenNthCalledWith(2, "http://localhost:8080/v1/llm", {
      method: "PUT",
      body: JSON.stringify(config),
    });
  });

  it("uses shared turn retry request for business retry execution", async () => {
    const request = vi.fn().mockResolvedValue({
      requestId: "req-1",
      messageId: "msg-2",
      sessionId: "session-1",
      turnId: "turn-2",
      status: "accepted",
    });
    const http: HttpAdapter = { request };
    const streamAdapter = noopStreamAdapter();
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.turns.retry({
      sessionId: "session-1",
      clientMessageId: "client-1",
      retryOfMessageId: "msg-1",
      input: { content: [{ type: "text", text: "retry" }] },
    });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/turns/retry",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          sessionId: "session-1",
          clientMessageId: "client-1",
          retryOfMessageId: "msg-1",
          input: { content: [{ type: "text", text: "retry" }] },
        }),
      }),
    );
  });

  it("uses the dedicated close route for session shutdown", async () => {
    const request = vi.fn().mockResolvedValue({ success: true });
    const http: HttpAdapter = { request };
    const streamAdapter = noopStreamAdapter();
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.sessions.close({ sessionId: "session-1" });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/sessions/session-1/close", {
      method: "POST",
    });
  });

  it("reads the active turn recovery snapshot", async () => {
    const request = vi.fn().mockResolvedValue({
      latestEventId: "ev_00000000000000000007",
      activeTurn: {
        identity: { sessionId: "session-1", turnId: "turn-1" },
        initialMessageId: "msg-1",
        state: "waitingForInput",
        lastEventId: "ev_00000000000000000007",
        updatedAt: 123,
      },
    });
    const http: HttpAdapter = { request };
    const client = new TalosClient(http, noopStreamAdapter(), "http://localhost:8080");

    const activeTurn = await client.sessions.activeTurn({ sessionId: "session-1" });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/active-turn",
    );
    expect(activeTurn.activeTurn).toMatchObject({
      identity: { sessionId: "session-1", turnId: "turn-1" },
      state: "waitingForInput",
    });
  });

  it("reads timeline recovery snapshots through the sessions SDK", async () => {
    const request = vi.fn().mockResolvedValue({
      replay: {
        events: [],
        highWaterMarkEventId: null,
        nextCursor: null,
        lastEventSeq: 0,
        cursorStatus: "fromBeginning",
      },
      activeTurn: null,
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    const response = await client.sessions.recoverTimeline({
      sessionId: "session-1",
      afterEventId: "ev_00000000000000000001",
      replayLimit: 100,
    });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/timeline/recovery?after_event_id=ev_00000000000000000001&limit=100",
    );
    expect(response.replay.events).toEqual([]);
  });

  it("posts project creation to the project collection route", async () => {
    const request = vi.fn().mockResolvedValue({
      project: {
        project_id: "project-1",
        name: "repo",
        directory: "/repo",
        created_at: 1,
        updated_at: 1,
        session_count: 0,
      },
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.projects.create({ name: "repo", directory: "/repo" });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/projects", {
      method: "POST",
      body: JSON.stringify({ name: "repo", directory: "/repo" }),
    });
  });

  it("sends project deletion to the project resource route", async () => {
    const request = vi.fn().mockResolvedValue({ success: true });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.projects.delete({ project_id: "project-1" });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/projects/project-1", {
      method: "DELETE",
    });
  });

  it("serializes session query parameters on the sessions list url", async () => {
    const request = vi.fn().mockResolvedValue({ results: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.sessions.list({ projectId: "project-1", limit: null, offset: null });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/sessions?projectId=project-1");
  });





  it("writes workspace docs through the workspace docs route", async () => {
    const request = vi.fn().mockResolvedValue({
      kind: "memory",
      path: "MEMORY.md",
      exists: true,
      content: "body",
      updated_at: 1,
      version_token: "v1",
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.workspaceDocs.put("memory", {
      content: "body",
      base_version: { mode: "must_not_exist" },
    });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/workspace/docs/memory", {
      method: "PUT",
      body: JSON.stringify({
        content: "body",
        base_version: { mode: "must_not_exist" },
      }),
    });
  });
});

function noopStreamAdapter(): StreamAdapter {
  return { stream: vi.fn().mockResolvedValue(undefined) };
}
