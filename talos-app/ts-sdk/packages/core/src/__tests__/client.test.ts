import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";
import type { HttpAdapter, StreamAdapter } from "../adapters";
import type { AgentEvent, StartTurnRequest, TimelineEvent } from "../types";

describe("TalosClient", () => {
  it("uses absolute urls for http requests", async () => {
    const request = vi.fn().mockResolvedValue({ results: [] });
    const http: HttpAdapter = { request };
    const streamAdapter = noopStreamAdapter();
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080/");

    await client.models.list();

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/models");
  });

  it("reads and writes llm config through the user config route", async () => {
    const request = vi.fn().mockResolvedValue({ provider: "anthropic", model: "claude-sonnet-4" });
    const http: HttpAdapter = { request };
    const streamAdapter = noopStreamAdapter();
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080/");
    const config = { provider: "anthropic", model: "claude-sonnet-4" } as never;

    await client.llm.get();
    await client.llm.update(config);

    expect(request).toHaveBeenNthCalledWith(1, "http://localhost:8080/v1/llm");
    expect(request).toHaveBeenNthCalledWith(2, "http://localhost:8080/v1/llm", {
      method: "PUT",
      body: JSON.stringify(config),
    });
  });

  it("uses ack plus events for streaming messages", async () => {
    const http: HttpAdapter = {
      request: vi.fn().mockResolvedValue({
        requestId: "req-1",
        messageId: "msg-1",
        sessionId: "session-1",
        turnId: "turn-1",
        status: "accepted",
      }),
    };
    const stream = vi.fn().mockImplementation(async (_url, _options, onMessage) => {
      onMessage(JSON.stringify(timelineEvent({
        identity: { sessionId: "session-1", turnId: "turn-1" },
        sequence: 1,
        payload: {
          type: "turnCompleted",
          data: { stopReason: "endTurn" },
        },
        occurredAt: 1,
      })));
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");
    const startRequest: StartTurnRequest = {
      sessionId: "session-1",
      input: { content: [{ type: "text", text: "hello" }] },
      modelSelection: null,
      userPermissionMode: null,
      projectId: null,
      currentDirectory: null,
      clientRequestId: null,
    };

    await client.turns.startEvents(
      startRequest,
      () => undefined,
    );

    expect(http.request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/turns/start",
      expect.objectContaining({
        method: "POST",
      }),
    );
    const [, startOptions] = vi.mocked(http.request).mock.calls[0];
    expect(JSON.parse(String(startOptions?.body))).toEqual({
      ...startRequest,
      clientRequestId: expect.any(String),
    });
    expect(stream).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/events",
      expect.objectContaining({
        method: "GET",
      }),
      expect.any(Function),
    );
  });

  it("filters timeline events by turn id in the SDK helper", async () => {
    const http: HttpAdapter = {
      request: vi.fn(),
    };
    const onEvent = vi.fn();
    const stream = vi.fn().mockImplementation(async (_url, options, onMessage) => {
      onMessage(JSON.stringify(timelineEvent({
        identity: { sessionId: "session-1", turnId: "turn-other" },
        sequence: 1,
        payload: {
          type: "turnStarted",
          data: {},
        },
        occurredAt: 1,
      })));
      onMessage(JSON.stringify(timelineEvent({
        identity: { sessionId: "session-1", turnId: "turn-1" },
        sequence: 2,
        payload: {
          type: "turnCompleted",
          data: { stopReason: "endTurn" },
        },
        occurredAt: 2,
      })));
      expect(options.signal.aborted).toBe(true);
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.turns.eventsForTurn("session-1", "turn-1", onEvent);

    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({
      eventId: "ev_00000000000000000002",
      event: { type: "agent", data: expect.objectContaining({
        identity: { sessionId: "session-1", turnId: "turn-1" },
        payload: { type: "turnCompleted", data: { stopReason: "endTurn" } },
      }) },
    }));
  });

  it("keeps timeline streams persistent only when explicitly requested", async () => {
    const http: HttpAdapter = { request: vi.fn() };
    const stream = vi.fn().mockResolvedValue(undefined);
    const client = new TalosClient(http, { stream }, "http://localhost:8080");

    await client.turns.events("session-1", undefined, vi.fn(), undefined, { persistent: true });

    expect(stream.mock.calls[0]?.[0]).toBe(
      "http://localhost:8080/v1/sessions/session-1/events?close_when_idle=false",
    );
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
      activeTurn: {
        identity: { sessionId: "session-1", turnId: "turn-1" },
        initialMessageId: "msg-1",
        state: "waitingForInput",
        lastEventId: "ev_00000000000000000007",
        updatedAt: 123,
      },
    });
    const http: HttpAdapter = { request };
    const streamAdapter = noopStreamAdapter();
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

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

  it("reads and writes memory files through request objects", async () => {
    const request = vi.fn().mockResolvedValue({ file: { date: "2026-06-24" } });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.memory.get({ date: "2026-06-24" });
    await client.memory.put({
      date: "2026-06-24",
      content: "# Memory",
      expected_version: "memory-token",
    });

    expect(request).toHaveBeenNthCalledWith(
      1,
      "http://localhost:8080/v1/memory/files/2026-06-24",
    );
    expect(request).toHaveBeenNthCalledWith(
      2,
      "http://localhost:8080/v1/memory/files/2026-06-24",
      {
        method: "PUT",
        body: JSON.stringify({
          date: "2026-06-24",
          content: "# Memory",
          expected_version: "memory-token",
        }),
      },
    );
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

function timelineEvent(event: AgentEvent): TimelineEvent {
  return {
    eventId: `ev_${String(event.sequence).padStart(20, "0")}`,
    eventSeq: event.sequence,
    persistedAt: event.occurredAt,
    event: { type: "agent", data: event },
  };
}

function noopStreamAdapter(): StreamAdapter {
  return { stream: vi.fn().mockResolvedValue(undefined) };
}
