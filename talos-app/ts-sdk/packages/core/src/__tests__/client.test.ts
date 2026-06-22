import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";
import type { HttpAdapter, StreamAdapter } from "../adapters";
import type { TalosTimelineEvent, TimelineEventEnvelope } from "../types";

describe("TalosClient", () => {
  it("uses absolute urls for http requests", async () => {
    const request = vi.fn().mockResolvedValue({ results: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080/");

    await client.models.list();

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/models");
  });

  it("reads and writes llm config through the user config route", async () => {
    const request = vi.fn().mockResolvedValue({ provider: "anthropic", model: "claude-sonnet-4" });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
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
        request_id: "req-1",
        message_id: "msg-1",
        session_id: "session-1",
        turn_id: "turn-1",
        status: "accepted",
      }),
    };
    const stream = vi.fn().mockImplementation(async (_url, _options, onMessage) => {
      onMessage(JSON.stringify(timelineEnvelope({
        event_seq: 1,
        event_id: "ev_00000000000000000001",
        session_id: "session-1",
        turn_id: "turn-1",
        message_id: "msg-1",
        sequence: 1,
        event_type: "turn_completed",
        payload: {
          type: "turn_completed",
          request_id: "req-1",
          turn_id: "turn-1",
          completed_at_ms: 1 as never,
          stop_reason: "end_turn",
          usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0, cache_creation_input_tokens: null, cache_read_input_tokens: null },
          metrics: null,
        },
        created_at_ms: 1,
      })));
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.turns.startEvents(
      {
        session_id: "session-1",
        input: [{ type: "text", text: "hello" }],
      },
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
      session_id: "session-1",
      input: [{ type: "text", text: "hello" }],
      client_request_id: expect.any(String),
    });
    expect(stream).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/events",
      expect.objectContaining({
        method: "GET",
      }),
      expect.any(Function),
    );
  });

  it("filters timeline events by message id in the SDK helper", async () => {
    const http: HttpAdapter = {
      request: vi.fn(),
    };
    const onEvent = vi.fn();
    const stream = vi.fn().mockImplementation(async (_url, options, onMessage) => {
      onMessage(JSON.stringify(timelineEnvelope({
        event_seq: 1,
        event_id: "ev_00000000000000000001",
        session_id: "session-1",
        turn_id: "turn-1",
        message_id: "other-message",
        sequence: 1,
        event_type: "item_delta",
        payload: {
          type: "item_delta",
          request_id: "req-1",
          turn_id: "turn-1",
          item_id: "assistant-1",
          delta: { type: "text", text: "skip" },
        },
        created_at_ms: 1,
      })));
      onMessage(JSON.stringify(timelineEnvelope({
        event_seq: 2,
        event_id: "ev_00000000000000000002",
        session_id: "session-1",
        turn_id: "turn-1",
        message_id: "msg-1",
        sequence: 2,
        event_type: "turn_completed",
        payload: {
          type: "turn_completed",
          request_id: "req-1",
          turn_id: "turn-1",
          completed_at_ms: 2 as never,
          stop_reason: "end_turn",
          usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0, cache_creation_input_tokens: null, cache_read_input_tokens: null },
          metrics: null,
        },
        created_at_ms: 2,
      })));
      expect(options.signal.aborted).toBe(true);
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.turns.eventsForMessage("session-1", "msg-1", onEvent);

    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({
      event_id: "ev_00000000000000000002",
      message_id: "msg-1",
      event: expect.objectContaining({
        event_type: "turn_completed",
        message_id: "msg-1",
      }),
    }));
  });

  it("uses shared turn retry request for business retry execution", async () => {
    const request = vi.fn().mockResolvedValue({
      request_id: "req-1",
      message_id: "msg-2",
      session_id: "session-1",
      turn_id: "turn-2",
      status: "accepted",
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.turns.retry({
      session_id: "session-1",
      client_message_id: "client-1",
      retry_of_message_id: "msg-1",
      input: [{ type: "text", text: "retry" }],
    });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/turns/retry",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          session_id: "session-1",
          client_message_id: "client-1",
          retry_of_message_id: "msg-1",
          input: [{ type: "text", text: "retry" }],
        }),
      }),
    );
  });

  it("uses the dedicated close route for session shutdown", async () => {
    const request = vi.fn().mockResolvedValue({ success: true });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.sessions.close("session-1");

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/sessions/session-1/close", {
      method: "POST",
    });
  });

  it("reads the active turn recovery snapshot", async () => {
    const request = vi.fn().mockResolvedValue({
      active_turn: {
        session_id: "session-1",
        turn_id: "turn-1",
        message_id: "msg-1",
        status: "waiting_user_action",
        last_event_id: "ev_00000000000000000007",
        pending_user_action: {
          type: "dynamic_tool_result",
          call_id: "call-1",
          tool_name: "ExternalTool",
        },
        updated_at: 123,
      },
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    const activeTurn = await client.sessions.activeTurn("session-1");

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/active-turn",
    );
    expect(activeTurn.active_turn.pending_user_action).toMatchObject({
      type: "dynamic_tool_result",
      call_id: "call-1",
    });
  });

  it("reads timeline recovery snapshots through the sessions SDK", async () => {
    const request = vi.fn().mockResolvedValue({
      snapshot: {
        replay: {
          events: [],
          high_water_mark_event_id: null,
          last_replay_seq: 0,
          cursor_status: "from_beginning",
        },
        active_turn: {
          session_id: "session-1",
          turn_id: null,
          message_id: null,
          status: "completed",
          last_event_id: null,
          pending_user_action: null,
          updated_at: 0,
        },
      },
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    const response = await client.sessions.recover(
      "session-1",
      "ev_00000000000000000001",
      100,
    );

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/recovery?after_event_id=ev_00000000000000000001&limit=100",
    );
    expect(response.snapshot.replay.events).toEqual([]);
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

    await client.projects.delete("project-1");

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

    await client.sessions.list({ project_id: "project-1", limit: null, offset: null });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/sessions?project_id=project-1");
  });

  it("serializes memory search query parameters on the request url", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.memory.search({
      q: "hello",
      limit: 5,
      date_from: "2026-04-01",
      date_to: "2026-04-30",
    });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/memory/search?q=hello&limit=5&date_from=2026-04-01&date_to=2026-04-30",
    );
  });

  it("forwards abort signals to memory search requests", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");
    const controller = new AbortController();

    await client.memory.search({ q: "hello" }, { signal: controller.signal });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/memory/search?q=hello",
      { signal: controller.signal },
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

function timelineEnvelope(event: TalosTimelineEvent): TimelineEventEnvelope {
  return {
    event_id: event.event_id ?? "ev_00000000000000000000",
    event_seq: event.event_seq ?? 0,
    created_at_ms: event.created_at_ms ?? 0,
    session_id: event.session_id,
    turn_id: event.turn_id,
    message_id: event.message_id,
    event,
  };
}
